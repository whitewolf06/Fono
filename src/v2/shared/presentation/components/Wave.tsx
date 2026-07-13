export function Wave() {
  return (
    <svg className="v2-wave" viewBox="0 0 900 220" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id="v2-wave-gradient" x1="0" x2="1">
          <stop offset="0" stopColor="#1478c8" stopOpacity="0" />
          <stop offset=".16" stopColor="#35baff" />
          <stop offset=".5" stopColor="#d6fbff" />
          <stop offset=".84" stopColor="#35baff" />
          <stop offset="1" stopColor="#1478c8" stopOpacity="0" />
        </linearGradient>
      </defs>
      <g className="v2-wave__spikes">
        <line x1="136" y1="24" x2="136" y2="196" />
        <line x1="158" y1="62" x2="158" y2="158" />
        <line x1="276" y1="40" x2="276" y2="180" />
        <line x1="625" y1="37" x2="625" y2="183" />
        <line x1="768" y1="22" x2="768" y2="198" />
      </g>
      <path className="v2-wave__line" d="M0 110 C60 110 64 106 92 106 C124 106 133 78 164 78 C196 78 205 123 241 123 C278 123 287 91 320 91 C352 91 366 113 397 113 C426 113 438 102 462 102 C488 102 501 116 532 116 C564 116 579 89 611 89 C646 89 655 124 692 124 C731 124 741 79 772 79 C805 79 813 109 900 109" />
    </svg>
  );
}
