const meta = import.meta.url;
const view = (
  <div className="root" data-count={42}>
    <span>{meta}</span>
    {[1, 2].map((n) => (
      <i key={n}>{n}</i>
    ))}
  </div>
);
export { view, meta };
