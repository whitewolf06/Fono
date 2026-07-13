import type { ReactNode } from "react";

interface PageFrameProps {
  icon: ReactNode;
  title: string;
  description: string;
  children: ReactNode;
}

export function PageFrame({
  icon,
  title,
  description,
  children,
}: PageFrameProps) {
  return (
    <section className="v2-page-frame">
      <header className="v2-page-frame__header">
        <span className="v2-page-frame__icon">{icon}</span>
        <div>
          <h1>{title}</h1>
          <p>{description}</p>
        </div>
      </header>
      {children}
    </section>
  );
}
