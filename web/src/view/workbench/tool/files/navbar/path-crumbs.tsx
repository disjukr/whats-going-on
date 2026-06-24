import React, { FormEvent, useEffect, useRef, useState } from "react";
import { ChevronRight } from "lucide-react";
import { pathCrumbs } from "../../../../../state/explorer.ts";

interface PathCrumbsProps {
  path?: string;
  onNavigate: (path?: string) => void;
}

const pathInputFormClassName = [
  "min-w-0",
  "[&_input]:w-full [&_input]:h-[2em] [&_input]:min-h-[2em]",
  "[&_input]:box-border [&_input]:px-[6px] [&_input]:leading-[1.6]",
  "[&_input:focus]:outline-none",
].join(" ");
const crumbsClassName = [
  "flex items-center gap-0 w-full h-[2em] min-h-[2em] min-w-0",
  "box-border leading-[1.6]",
  "overflow-x-auto overflow-y-hidden overscroll-x-contain [scrollbar-width:none] cursor-text",
  "[&::-webkit-scrollbar]:hidden",
  "[&_svg]:flex-[0_0_auto] [&_svg]:pointer-events-none",
  "[&_button]:inline-flex [&_button]:appearance-none [&_button]:cursor-pointer",
  "[&_button]:items-center [&_button]:flex-[0_0_auto] [&_button]:[font-family:inherit]",
  "[&_button]:min-w-0 [&_button]:max-w-[180px]",
  "[&_button]:h-[2em] [&_button]:min-h-[2em] [&_button]:overflow-hidden [&_button]:leading-[1.6]",
  "[&_button]:box-border [&_button]:border-transparent [&_button]:bg-transparent [&_button]:px-[6px]",
  "[&_button:hover]:bg-[#eef3fb] [&_button:hover]:text-[#20242d]",
  "[&_button:focus-visible]:bg-[#eef3fb] [&_button:focus-visible]:outline-none",
  "[&_button]:text-ellipsis [&_button]:whitespace-nowrap",
].join(" ");

export function PathCrumbs(
  { path, onNavigate }: PathCrumbsProps,
) {
  const [editing, setEditing] = useState(false);
  const [draftPath, setDraftPath] = useState(path ?? "");
  const inputRef = useRef<HTMLInputElement>(null);
  const crumbsRef = useRef<HTMLDivElement>(null);
  const crumbs = pathCrumbs(path);

  useEffect(() => {
    if (!editing) setDraftPath(path ?? "");
  }, [editing, path]);

  useEffect(() => {
    if (!editing) return;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [editing]);

  useEffect(() => {
    if (editing) return;
    const element = crumbsRef.current;
    if (!element) return;
    const frame = requestAnimationFrame(() => {
      element.scrollLeft = element.scrollWidth;
    });
    return () => cancelAnimationFrame(frame);
  }, [editing, path]);

  useEffect(() => {
    if (editing) return;
    const element = crumbsRef.current;
    if (!element) return;
    const crumbsElement = element;

    function scrollCrumbsHorizontally(event: WheelEvent) {
      if (crumbsElement.scrollWidth <= crumbsElement.clientWidth) return;
      const delta = event.deltaX || event.deltaY;
      if (delta === 0) return;
      event.preventDefault();
      crumbsElement.scrollLeft += delta;
    }

    crumbsElement.addEventListener("wheel", scrollCrumbsHorizontally, {
      passive: false,
    });
    return () => {
      crumbsElement.removeEventListener("wheel", scrollCrumbsHorizontally);
    };
  }, [editing]);

  function beginEditing() {
    setDraftPath(path ?? "");
    setEditing(true);
  }

  function cancelEditing() {
    setDraftPath(path ?? "");
    setEditing(false);
  }

  function submitPath(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const nextPath = draftPath.trim();
    setEditing(false);
    onNavigate(nextPath || undefined);
  }

  if (editing) {
    return (
      <form className={pathInputFormClassName} onSubmit={submitPath}>
        <input
          ref={inputRef}
          value={draftPath}
          onChange={(event) => setDraftPath(event.target.value)}
          onBlur={cancelEditing}
          onKeyDown={(event) => {
            if (event.key === "Escape") cancelEditing();
          }}
          aria-label="Path"
          placeholder="Path"
        />
      </form>
    );
  }

  return (
    <div
      ref={crumbsRef}
      className={crumbsClassName}
      aria-label="Path"
      onMouseDown={(event) => {
        if (event.target !== event.currentTarget) return;
        event.preventDefault();
        beginEditing();
      }}
    >
      {crumbs.map((crumb, index) => (
        <React.Fragment key={`${crumb.path ?? "roots"}:${index}`}>
          {index > 0 ? <ChevronRight size={12} /> : null}
          <button
            type="button"
            onClick={() =>
              onNavigate(crumb.path)}
          >
            {crumb.label}
          </button>
        </React.Fragment>
      ))}
    </div>
  );
}
