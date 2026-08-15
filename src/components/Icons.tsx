// Inline SVG icons: no dependencies and no icon-font cost.
import type { SVGProps } from "react";

const base = (p: SVGProps<SVGSVGElement>) => ({
  width: 15,
  height: 15,
  viewBox: "0 0 24 24",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.9,
  strokeLinecap: "round" as const,
  strokeLinejoin: "round" as const,
  ...p,
});

export const IconPlus = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M12 5v14M5 12h14" />
  </svg>
);

export const IconChevron = ({ open, ...p }: SVGProps<SVGSVGElement> & { open?: boolean }) => (
  <svg {...base(p)} style={{ transform: open ? "none" : "rotate(-90deg)", transition: "transform .12s" }}>
    <path d="M6 9l6 6 6-6" />
  </svg>
);

/** Settings: sliders. A gear at 15 px turns into a blob (or into a sun, if
 *  simplified); two sliders always read correctly. */
export const IconGear = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base({ strokeWidth: 2, ...p })}>
    <path d="M9 3.5v17M15 3.5v17" />
    <circle cx="9" cy="8.5" r="2.7" fill="currentColor" stroke="none" />
    <circle cx="15" cy="15.5" r="2.7" fill="currentColor" stroke="none" />
  </svg>
);

export const IconStop = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="6" y="6" width="12" height="12" rx="1.5" />
  </svg>
);

export const IconPlay = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M7 4l12 8-12 8z" />
  </svg>
);

export const IconTrash = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />
  </svg>
);

export const IconRefresh = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M21 12a9 9 0 11-3-6.7M21 4v5h-5" />
  </svg>
);

export const IconTerminal = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M5 7l4 5-4 5M12 17h7" />
  </svg>
);

export const IconSearch = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-4.2-4.2" />
  </svg>
);

export const IconPanel = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="3" y="4" width="18" height="16" rx="2" />
    <path d="M9 4v16" />
  </svg>
);

export const IconChart = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3.5 20.5h17" />
    <path d="M6.5 20.5V13M11 20.5V6.5M15.5 20.5v-5M20 20.5V10" />
  </svg>
);

export const IconFolder = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M3 7a2 2 0 012-2h4l2 2h8a2 2 0 012 2v8a2 2 0 01-2 2H5a2 2 0 01-2-2z" />
  </svg>
);

export const IconX = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M6 6l12 12M18 6L6 18" />
  </svg>
);

export const IconMin = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base({ ...p, strokeWidth: 1.6 })}>
    <path d="M6 12h12" />
  </svg>
);

export const IconMax = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base({ ...p, strokeWidth: 1.6 })}>
    <rect x="6" y="6" width="12" height="12" rx="1.5" />
  </svg>
);

export const IconBranch = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="7" cy="6" r="2.2" />
    <circle cx="7" cy="18" r="2.2" />
    <circle cx="17" cy="12" r="2.2" />
    <path d="M7 8.2v7.6M9.2 6h3.3a2.3 2.3 0 012.3 2.3v1.5" />
  </svg>
);

export const IconEdit = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M4 20h4L20 8l-4-4L4 16z" />
    <path d="M14 6l4 4" />
  </svg>
);

export const IconCopy = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <rect x="9" y="9" width="11" height="11" rx="2" />
    <path d="M15 5.5A1.5 1.5 0 0013.5 4H5.5A1.5 1.5 0 004 5.5v8A1.5 1.5 0 005.5 15" />
  </svg>
);

export const IconSave = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <path d="M5 5h11l3 3v11H5z" />
    <path d="M9 5v5h6V5M9 19v-4h6v4" />
  </svg>
);

export const IconKey = (p: SVGProps<SVGSVGElement>) => (
  <svg {...base(p)}>
    <circle cx="8" cy="12" r="3.4" />
    <path d="M11.4 12H21M17.5 12v3M14.5 12v2.4" />
  </svg>
);

/** Eye toggle for showing/hiding the key; `crossed` draws a slash over it. */
export const IconEye = ({ crossed, ...p }: SVGProps<SVGSVGElement> & { crossed?: boolean }) => (
  <svg {...base(p)}>
    <path d="M2.5 12S6 5.8 12 5.8 21.5 12 21.5 12 18 18.2 12 18.2 2.5 12 2.5 12z" />
    <circle cx="12" cy="12" r="2.7" />
    {crossed && <path d="M4 20L20 4" />}
  </svg>
);
