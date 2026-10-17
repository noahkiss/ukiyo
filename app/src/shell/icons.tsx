// Line icons from the design canvas, 24-unit grid, stroked in currentColor.
import type { ReactNode } from "react";

function Svg({ size = 20, width = 1.7, children }: { size?: number; width?: number; children: ReactNode }) {
  return (
    <svg aria-hidden="true" width={size} height={size} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={width} strokeLinecap="round" strokeLinejoin="round">
      {children}
    </svg>
  );
}

export const HubIcon = () => (
  <Svg>
    <path d="M3 11l9-7 9 7" />
    <path d="M5 10v10h14V10" />
    <path d="M10 20v-6h4v6" />
  </Svg>
);

export const RecipesIcon = () => (
  <Svg>
    <path d="M12 3l9 5-9 5-9-5 9-5z" />
    <path d="M3 13l9 5 9-5" />
    <path d="M3 17l9 5 9-5" />
  </Svg>
);

export const SettingsIcon = () => (
  <Svg>
    <path d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0" />
    <circle cx="16" cy="6" r="2" />
    <circle cx="10" cy="12" r="2" />
    <circle cx="18" cy="18" r="2" />
  </Svg>
);

export const LabIcon = () => (
  <Svg>
    <path d="M9 3h6" />
    <path d="M10 3v6l-5 9a2 2 0 0 0 1.8 3h10.4a2 2 0 0 0 1.8-3l-5-9V3" />
    <path d="M7.5 14h9" />
  </Svg>
);

export const RefreshIcon = () => (
  <Svg size={18} width={1.8}>
    <path d="M20 11a8 8 0 1 0-2.3 5.7" />
    <path d="M20 4v7h-7" />
  </Svg>
);

export const SearchIcon = ({ size = 18 }: { size?: number }) => (
  <Svg size={size} width={1.8}>
    <circle cx="11" cy="11" r="7" />
    <path d="M20 20l-4-4" />
  </Svg>
);

export const ChevronDown = () => (
  <Svg size={16} width={2}>
    <path d="M6 9l6 6 6-6" />
  </Svg>
);

export const LockIcon = () => (
  <Svg size={18} width={1.8}>
    <rect x="5" y="11" width="14" height="10" rx="2" />
    <path d="M8 11V8a4 4 0 0 1 8 0v3" />
  </Svg>
);

export const LinkIcon = () => (
  <Svg size={18} width={1.8}>
    <path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1" />
    <path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1" />
  </Svg>
);

export const PhotoIcon = () => (
  <Svg size={18} width={1.8}>
    <rect x="3" y="5" width="18" height="14" rx="2" />
    <circle cx="9" cy="10" r="2" />
    <path d="M21 16l-5-5-9 8" />
  </Svg>
);

export const CameraIcon = () => (
  <Svg size={18} width={1.8}>
    <path d="M4 8h3l2-3h6l2 3h3v11H4z" />
    <circle cx="12" cy="13" r="3.5" />
  </Svg>
);

export const WarnIcon = () => (
  <Svg size={20} width={1.8}>
    <path d="M12 3l10 18H2z" />
    <path d="M12 10v5" />
    <path d="M12 18h0" />
  </Svg>
);

export const StarIcon = ({ filled }: { filled: boolean }) => (
  <svg aria-hidden="true" width="18" height="18" viewBox="0 0 24 24" fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.7" strokeLinejoin="round">
    <path d="M12 3.5l2.6 5.3 5.9.9-4.3 4.1 1 5.8L12 16.9l-5.2 2.7 1-5.8-4.3-4.1 5.9-.9z" />
  </svg>
);

export const PinIcon = ({ filled }: { filled: boolean }) => (
  <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
    <path d="M5 2h6l-1 4 2 3H4l2-3zM8 9v5" fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round" />
  </svg>
);

export const CloseIcon = () => (
  <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true">
    <path d="M4 4l8 8M12 4l-8 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
  </svg>
);
