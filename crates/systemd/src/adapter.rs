//! The systemd driven adapter, speaking D-Bus in-process.

use std::future::Future;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use gilfoyle_core::{
    JobResult, ServiceCatalog, ServiceControl, SystemdError, Unit, UnitActionOutcome, UnitFilter,
    UnitName, UnitStatus, UnitTimestamps, UnitType, micros_to_rfc3339,
};
use zbus::Connection;
use zbus::OwnedObjectPath;
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};

use crate::error::map_zbus_error;
use crate::mapping::{apply_filter, raw_to_unit};

/// Default timeout for a single D-Bus call.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(10);
/// Default timeout for waiting on a start/stop job.
pub const DEFAULT_JOB_TIMEOUT: Duration = Duration::from_secs(30);
/// Job mode used for start and stop, matching `systemctl`'s default.
const JOB_MODE: &str = "replace";

/// Wrap a D-Bus call in a timeout and map its error.
async fn timed<T>(
    timeout: Duration,
    unit: &str,
    future: impl Future<Output = zbus::Result<T>>,
) -> Result<T, SystemdError> {
    match tokio::time::timeout(timeout, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(map_zbus_error(error, unit)),
        Err(_) => Err(SystemdError::Timeout(unit.to_string())),
    }
}

/// systemd driven adapter. Holds a live bus connection; performs no process spawning.
pub struct SystemdAdapter {
    connection: Connection,
    call_timeout: Duration,
    job_timeout: Duration,
}

impl SystemdAdapter {
    /// Build an adapter over an existing bus connection.
    pub fn new(connection: Connection) -> Self {
        Self {
            connection,
            call_timeout: DEFAULT_CALL_TIMEOUT,
            job_timeout: DEFAULT_JOB_TIMEOUT,
        }
    }

    /// Connect to the system bus.
    pub async fn connect_system() -> Result<Self, SystemdError> {
        let connection = Connection::system()
            .await
            .map_err(|error| SystemdError::Unavailable(error.to_string()))?;
        Ok(Self::new(connection))
    }

    /// Override the call and job timeouts (used by tests).
    pub fn with_timeouts(mut self, call_timeout: Duration, job_timeout: Duration) -> Self {
        self.call_timeout = call_timeout;
        self.job_timeout = job_timeout;
        self
    }

    async fn manager(&self) -> Result<ManagerProxy<'_>, SystemdError> {
        ManagerProxy::new(&self.connection)
            .await
            .map_err(|error| map_zbus_error(error, ""))
    }

    async fn unit_path(&self, name: &UnitName) -> Result<OwnedObjectPath, SystemdError> {
        let manager = self.manager().await?;
        timed(
            self.call_timeout,
            name.as_str(),
            manager.get_unit(name.as_str().to_string()),
        )
        .await
    }

    async fn active_state(&self, name: &UnitName) -> Result<String, SystemdError> {
        let path = self.unit_path(name).await?;
        let proxy = UnitProxy::new(&self.connection, path)
            .await
            .map_err(|error| map_zbus_error(error, name.as_str()))?;
        timed(self.call_timeout, name.as_str(), proxy.active_state()).await
    }

    async fn run_job(
        &self,
        name: &UnitName,
        start: bool,
    ) -> Result<UnitActionOutcome, SystemdError> {
        let unit = name.as_str();
        let manager = self.manager().await?;
        let before = self.active_state(name).await?;

        // Subscribe before enqueuing so no JobRemoved signal can be missed.
        timed(self.call_timeout, unit, manager.subscribe()).await?;
        let signal_proxy = zbus::Proxy::new(
            &self.connection,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .await
        .map_err(|error| map_zbus_error(error, unit))?;
        let mut signals = signal_proxy
            .receive_signal("JobRemoved")
            .await
            .map_err(|error| map_zbus_error(error, unit))?;

        let job = if start {
            timed(
                self.call_timeout,
                unit,
                manager.start_unit(unit.to_string(), JOB_MODE.to_string()),
            )
            .await?
        } else {
            timed(
                self.call_timeout,
                unit,
                manager.stop_unit(unit.to_string(), JOB_MODE.to_string()),
            )
            .await?
        };

        let result = tokio::time::timeout(self.job_timeout, async {
            while let Some(message) = signals.next().await {
                let parsed = message
                    .body()
                    .deserialize::<(u32, OwnedObjectPath, String, String)>();
                if let Ok((_, signal_job, _, result)) = parsed
                    && signal_job == job
                {
                    return Some(result);
                }
            }
            None
        })
        .await;

        let result = match result {
            Ok(Some(result)) => result,
            Ok(None) => {
                return Err(SystemdError::Unavailable(
                    "job signal stream closed".to_string(),
                ));
            }
            Err(_) => return Err(SystemdError::Timeout(unit.to_string())),
        };

        let after = self.active_state(name).await?;
        Ok(UnitActionOutcome {
            unit: unit.to_string(),
            job_result: JobResult::new(result),
            active_state_before: before,
            active_state_after: after,
        })
    }
}

#[async_trait]
impl ServiceCatalog for SystemdAdapter {
    async fn list_units(&self, filter: &UnitFilter) -> Result<Vec<Unit>, SystemdError> {
        let manager = self.manager().await?;
        let raw = match &filter.active_state {
            Some(state) => {
                timed(
                    self.call_timeout,
                    "",
                    manager.list_units_filtered(vec![state.clone()]),
                )
                .await?
            }
            None => timed(self.call_timeout, "", manager.list_units()).await?,
        };
        let units: Vec<Unit> = raw.iter().map(raw_to_unit).collect();
        Ok(apply_filter(units, filter))
    }

    async fn unit_status(&self, name: &UnitName) -> Result<UnitStatus, SystemdError> {
        let unit = name.as_str();
        let path = self.unit_path(name).await?;
        let proxy = UnitProxy::new(&self.connection, path.clone())
            .await
            .map_err(|error| map_zbus_error(error, unit))?;

        let description = timed(self.call_timeout, unit, proxy.description()).await?;
        let load_state = timed(self.call_timeout, unit, proxy.load_state()).await?;
        let active_state = timed(self.call_timeout, unit, proxy.active_state()).await?;
        let sub_state = timed(self.call_timeout, unit, proxy.sub_state()).await?;
        let unit_file_state = timed(self.call_timeout, unit, proxy.unit_file_state())
            .await
            .ok();
        let can_start = timed(self.call_timeout, unit, proxy.can_start()).await.ok();
        let can_stop = timed(self.call_timeout, unit, proxy.can_stop()).await.ok();

        let state_change = micros_to_rfc3339(
            timed(self.call_timeout, unit, proxy.state_change_timestamp()).await?,
        );
        let active_enter = micros_to_rfc3339(
            timed(self.call_timeout, unit, proxy.active_enter_timestamp()).await?,
        );
        let inactive_enter = micros_to_rfc3339(
            timed(self.call_timeout, unit, proxy.inactive_enter_timestamp()).await?,
        );

        let (main_pid, result, exec_main_start, exec_main_exit) =
            if name.unit_type() == UnitType::Service {
                let service = ServiceProxy::new(&self.connection, path)
                    .await
                    .map_err(|error| map_zbus_error(error, unit))?;
                let main_pid = timed(self.call_timeout, unit, service.main_pid()).await?;
                let result = timed(self.call_timeout, unit, service.result()).await?;
                let start = micros_to_rfc3339(
                    timed(self.call_timeout, unit, service.exec_main_start_timestamp()).await?,
                );
                let exit = micros_to_rfc3339(
                    timed(self.call_timeout, unit, service.exec_main_exit_timestamp()).await?,
                );
                (Some(main_pid), Some(result), start, exit)
            } else {
                (None, None, None, None)
            };

        Ok(UnitStatus {
            name: unit.to_string(),
            description,
            load_state,
            active_state,
            sub_state,
            unit_file_state,
            can_start,
            can_stop,
            main_pid,
            result,
            timestamps: UnitTimestamps {
                state_change,
                active_enter,
                inactive_enter,
                exec_main_start,
                exec_main_exit,
            },
        })
    }
}

#[async_trait]
impl ServiceControl for SystemdAdapter {
    async fn start_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.run_job(name, true).await
    }

    async fn stop_unit(&self, name: &UnitName) -> Result<UnitActionOutcome, SystemdError> {
        self.run_job(name, false).await
    }
}
