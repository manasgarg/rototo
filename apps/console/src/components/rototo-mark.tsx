export function RototoMark({ size = 26 }: { size?: number }) {
  return (
    <svg
      aria-label="rototo mark"
      fill="none"
      height={size}
      role="img"
      viewBox="0 0 48 48"
      width={size}
    >
      <rect height="16" rx="8" stroke="currentColor" strokeWidth="3" width="30" x="9" y="16" />
      <circle cx="31.5" cy="24" fill="currentColor" r="4.6" />
    </svg>
  );
}
