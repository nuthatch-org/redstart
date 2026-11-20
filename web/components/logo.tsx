// A redstart is a small songbird whose signature is its rusty-red tail
// ("start" = old English for tail). Perched-bird mark with a lighter tail.
export function Bird({ className = "" }: { className?: string }) {
  return (
    <svg viewBox="0 0 32 32" aria-hidden className={className}>
      {/* rusty tail */}
      <path
        d="M15.6 21 L13.9 28.4 L19.4 26.8 L18.4 20.4 Z"
        fill="var(--color-accent-ink)"
      />
      {/* head */}
      <circle cx="13.4" cy="11" r="4.7" fill="currentColor" />
      {/* body */}
      <path
        d="M9.4 14 C8.9 18.4 11.4 22.4 15.4 22.4 C19.7 22.4 21.2 17.9 19.8 14.1 C18.7 11.5 15.8 10.3 13.4 11.3 Z"
        fill="currentColor"
      />
      {/* beak */}
      <path d="M9.2 10.2 L4.4 9.7 L9.4 12.3 Z" fill="currentColor" />
      {/* eye */}
      <circle cx="11.5" cy="10" r="1.15" fill="var(--color-bg)" />
    </svg>
  );
}

export function Wordmark({ className = "" }: { className?: string }) {
  return (
    <span className={`inline-flex items-center gap-2 ${className}`}>
      <Bird className="h-[1.15em] w-[1.15em] text-accent" />
      <span className="font-display font-semibold tracking-[-0.06em]">redstart</span>
    </span>
  );
}
