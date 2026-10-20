// A thin pill scrollbar like the one on magmamcsr.com, instead of the native Windows one.
// The scroll container keeps scrolling natively (wheel, touch, keyboard); its own
// scrollbar is hidden and a track with a draggable thumb is placed right after it.
// The container's parent should lay out in a row (display: flex).
import "./scrollbar.css";

/** Attach the scrollbar to `container`. Returns a function that removes it. */
export function customScrollbar(container: HTMLElement): () => void {
  container.classList.add("scroll-hidden");
  const track = document.createElement("div");
  track.className = "scroll-track";
  const thumb = document.createElement("div");
  thumb.className = "scroll-thumb";
  track.append(thumb);
  container.after(track);

  const update = () => {
    const { scrollHeight, clientHeight, scrollTop } = container;
    const overflow = scrollHeight > clientHeight + 1;
    track.hidden = !overflow;
    if (!overflow) return;
    const height = Math.max((clientHeight / scrollHeight) * 100, 8);
    thumb.style.height = `${height}%`;
    thumb.style.top = `${(scrollTop / (scrollHeight - clientHeight)) * (100 - height)}%`;
  };

  // Clicking the track jumps there; the thumb itself is dragged.
  track.addEventListener("mousedown", (e) => {
    if (e.target !== track) return;
    const rect = track.getBoundingClientRect();
    const ratio = (e.clientY - rect.top) / rect.height;
    container.scrollTop = ratio * container.scrollHeight - container.clientHeight / 2;
  });
  thumb.addEventListener("mousedown", (e) => {
    e.preventDefault();
    const startY = e.clientY;
    const startTop = container.scrollTop;
    const scale = container.scrollHeight / track.getBoundingClientRect().height;
    thumb.classList.add("dragging");
    const move = (m: MouseEvent) => (container.scrollTop = startTop + (m.clientY - startY) * scale);
    const up = () => {
      thumb.classList.remove("dragging");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", up);
    };
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", up);
  });
  track.addEventListener("wheel", (e) => container.scrollBy({ top: e.deltaY }), { passive: true });

  // Content changes size (players added, a section opened): recompute.
  container.addEventListener("scroll", update, { passive: true });
  const resize = new ResizeObserver(update);
  resize.observe(container);
  const watchChildren = () => {
    for (const child of container.children) resize.observe(child);
    update();
  };
  const mutations = new MutationObserver(watchChildren);
  mutations.observe(container, { childList: true, subtree: true });
  watchChildren();

  return () => {
    resize.disconnect();
    mutations.disconnect();
    container.removeEventListener("scroll", update);
    container.classList.remove("scroll-hidden");
    track.remove();
  };
}
