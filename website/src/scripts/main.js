// Gilfoyle — the only motion on this page, and it is deliberately joyless.

const reduce =
  typeof window !== "undefined" &&
  window.matchMedia("(prefers-reduced-motion: reduce)").matches;

// Typewriter for [data-type] elements.
for (const el of document.querySelectorAll("[data-type]")) {
  const text = el.getAttribute("data-type") || "";
  if (reduce) {
    el.textContent = text;
    continue;
  }
  el.textContent = "";
  const caret = document.createElement("span");
  caret.className = "caret";
  el.after(caret);
  let i = 0;
  const tick = () => {
    el.textContent = text.slice(0, i++);
    if (i <= text.length) {
      window.setTimeout(tick, 34 + Math.random() * 46);
    } else {
      window.setTimeout(() => caret.remove(), 900);
    }
  };
  window.setTimeout(tick, 350);
}

// Reveal on scroll.
if (!reduce && "IntersectionObserver" in window) {
  const io = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) {
        if (entry.isIntersecting) {
          entry.target.classList.add("is-in");
          io.unobserve(entry.target);
        }
      }
    },
    { threshold: 0.12, rootMargin: "0px 0px -8% 0px" },
  );
  for (const el of document.querySelectorAll("[data-reveal]")) io.observe(el);
} else {
  for (const el of document.querySelectorAll("[data-reveal]"))
    el.classList.add("is-in");
}
