import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { Icon } from "./ui";

// Mount in the current dialog's top layer, not in the clipped scrolling form.
export function Popup({
  anchor,
  menuRef,
  children,
  id,
  label,
  tooltip = false,
}: {
  anchor: RefObject<HTMLElement | null>;
  menuRef: RefObject<HTMLDivElement | null>;
  children: ReactNode;
  id: string;
  label?: string;
  tooltip?: boolean;
}) {
  const [style, setStyle] = useState<CSSProperties>({ visibility: "hidden" });
  useLayoutEffect(() => {
    const update = () => {
      const trigger = anchor.current,
        menu = menuRef.current;
      if (!trigger || !menu) return;
      const r = trigger.getBoundingClientRect(),
        margin = 12,
        gap = 6;
      const width = Math.min(tooltip ? 300 : r.width, innerWidth - margin * 2);
      const below = innerHeight - r.bottom - margin - gap;
      const above = r.top - margin - gap;
      const desired = Math.min(tooltip ? 200 : 280, menu.scrollHeight || 280);
      const up = below < desired && above > below;
      const maxHeight = Math.max(
        24,
        Math.min(tooltip ? 200 : 280, up ? above : below),
      );
      const height = Math.min(desired, maxHeight);
      setStyle({
        position: "fixed",
        width,
        left: Math.max(margin, Math.min(r.left, innerWidth - width - margin)),
        top: up ? Math.max(margin, r.top - height - gap) : r.bottom + gap,
        maxHeight,
        visibility: "visible",
      });
    };
    update();
    const observer = new ResizeObserver(update);
    if (anchor.current) observer.observe(anchor.current);
    if (menuRef.current) observer.observe(menuRef.current);
    window.addEventListener("resize", update);
    window.addEventListener("scroll", update, true);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", update);
      window.removeEventListener("scroll", update, true);
    };
  }, [anchor, menuRef, tooltip]);
  return createPortal(
    <div
      ref={menuRef}
      id={id}
      role={tooltip ? "tooltip" : "listbox"}
      aria-label={label}
      className={tooltip ? "ui-tooltip" : "choice-menu"}
      style={style}
    >
      {children}
    </div>,
    anchor.current?.closest("dialog") ?? document.body,
  );
}

export function useDismiss(
  open: boolean,
  root: RefObject<HTMLElement | null>,
  menu: RefObject<HTMLElement | null>,
  close: () => void,
) {
  useEffect(() => {
    if (!open) return;
    const outside = (e: Event) => {
      if (
        !root.current?.contains(e.target as Node) &&
        !menu.current?.contains(e.target as Node)
      )
        close();
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("focusin", outside);
    return () => {
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("focusin", outside);
    };
  }, [open, root, menu, close]);
}

type Option = {
  value: string;
  label: string;
  icon?: string;
  disabled?: boolean;
};
export function Select({
  label,
  value,
  options,
  onChange,
  disabled = false,
}: {
  label: string;
  value: string;
  options: Option[];
  onChange: (value: string) => void;
  disabled?: boolean;
}) {
  const root = useRef<HTMLDivElement>(null),
    trigger = useRef<HTMLButtonElement>(null),
    menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false),
    [active, setActive] = useState(0);
  const id = useId(),
    selected = options.find((o) => o.value === value);
  useDismiss(open, root, menu, () => setOpen(false));
  useEffect(() => {
    if (disabled) setOpen(false);
  }, [disabled]);
  useEffect(() => {
    if (open)
      menu.current
        ?.querySelector(`[data-index="${active}"]`)
        ?.scrollIntoView({ block: "nearest" });
  }, [active, open]);
  function show() {
    setActive(
      Math.max(
        0,
        options.findIndex((o) => o.value === value && !o.disabled),
      ),
    );
    setOpen(true);
  }
  function move(step: number) {
    if (!open) {
      show();
      return;
    }
    for (let n = 1; n <= options.length; n++) {
      const i = (active + step * n + options.length * 2) % options.length;
      if (!options[i].disabled) {
        setActive(i);
        break;
      }
    }
  }
  function choose(i: number) {
    if (!options[i] || options[i].disabled) return;
    onChange(options[i].value);
    setOpen(false);
    trigger.current?.focus();
  }
  return (
    <div ref={root} className="select-control">
      <button
        ref={trigger}
        type="button"
        className="select-trigger"
        role="combobox"
        aria-label={label}
        aria-haspopup="listbox"
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-activedescendant={open ? `${id}-${active}` : undefined}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show())}
        onKeyDown={(e) => {
          if (e.nativeEvent.isComposing) return;
          if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            move(e.key === "ArrowDown" ? 1 : -1);
          }
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            open ? choose(active) : show();
          }
          if (e.key === "Escape" && open) {
            e.preventDefault();
            e.stopPropagation();
            setOpen(false);
          }
          if (e.key === "Tab") setOpen(false);
        }}
      >
        <span>
          {selected?.icon && <Icon name={selected.icon} size={16} />}
          {selected?.label ?? value}
        </span>
        <Icon name="chevron-down" size={14} />
      </button>
      {open && (
        <Popup anchor={trigger} menuRef={menu} id={id} label={label}>
          {options.map((o, i) => (
            <button
              key={o.value}
              type="button"
              id={`${id}-${i}`}
              role="option"
              tabIndex={-1}
              aria-selected={o.value === value}
              disabled={o.disabled}
              data-index={i}
              data-active={i === active || undefined}
              onMouseDown={(e) => e.preventDefault()}
              onMouseEnter={() => !o.disabled && setActive(i)}
              onClick={() => choose(i)}
            >
              {o.icon && <Icon name={o.icon} size={16} />}
              <span>{o.label}</span>
              {o.value === value && <Icon name="check" size={14} />}
            </button>
          ))}
        </Popup>
      )}
    </div>
  );
}

export function ModelCombobox({
  value,
  options,
  onChange,
  onSelect,
  inputRef,
  openSignal,
  busy,
  children,
}: {
  value: string;
  options: string[];
  onChange: (value: string) => void;
  onSelect: (value: string) => void;
  inputRef: RefObject<HTMLInputElement | null>;
  openSignal: number;
  busy: boolean;
  children: ReactNode;
}) {
  const root = useRef<HTMLDivElement>(null),
    anchor = useRef<HTMLDivElement>(null),
    menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false),
    [showAll, setShowAll] = useState(false),
    [active, setActive] = useState(0);
  const id = useId();
  const filtered = showAll
    ? options
    : options.filter((o) =>
        o.toLowerCase().includes(value.trim().toLowerCase()),
      );
  useDismiss(open, root, menu, () => setOpen(false));
  useEffect(() => {
    setOpen(false);
  }, [options]);
  useEffect(() => {
    if (openSignal > 0) {
      inputRef.current?.focus();
      setShowAll(true);
      setActive(0);
      setOpen(true);
    }
  }, [openSignal, inputRef]);
  useEffect(() => {
    if (busy) setOpen(false);
  }, [busy]);
  useEffect(() => {
    if (open)
      menu.current
        ?.querySelector(`[data-index="${active}"]`)
        ?.scrollIntoView({ block: "nearest" });
  }, [active, open]);
  function choose(v: string) {
    onSelect(v);
    inputRef.current?.focus();
    setOpen(false);
  }
  return (
    <div ref={root} className="model-combobox">
      <div ref={anchor}>
        <input
          ref={inputRef}
          name="model"
          role="combobox"
          aria-label="模型名称"
          aria-haspopup="listbox"
          aria-autocomplete="list"
          aria-expanded={open}
          aria-controls={open ? id : undefined}
          aria-activedescendant={
            open && filtered[active] ? `${id}-${active}` : undefined
          }
          required
          maxLength={200}
          value={value}
          onChange={(e) => {
            onChange(e.target.value);
            setShowAll(false);
            setActive(0);
            setOpen(options.length > 0);
          }}
          onFocus={() => {
            if (options.length && !busy) {
              setShowAll(false);
              setActive(0);
              setOpen(true);
            }
          }}
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing) return;
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              if (!open) {
                setOpen(options.length > 0);
                setShowAll(false);
                setActive(0);
              } else if (filtered.length)
                setActive(
                  (i) =>
                    (i + (e.key === "ArrowDown" ? 1 : -1) + filtered.length) %
                    filtered.length,
                );
            }
            if (e.key === "Enter" && open) {
              e.preventDefault();
              if (filtered[active]) choose(filtered[active]);
            }
            if (e.key === "Escape" && open) {
              e.preventDefault();
              e.stopPropagation();
              setOpen(false);
            }
            if (e.key === "Tab") setOpen(false);
          }}
        />
        {children}
        {options.length > 0 && (
          <button
            type="button"
            aria-label="展开模型列表"
            aria-expanded={open}
            onMouseDown={(e) => e.preventDefault()}
            onClick={() => {
              if (open) setOpen(false);
              else {
                inputRef.current?.focus();
                setShowAll(true);
                setActive(0);
                setOpen(true);
              }
            }}
          >
            <Icon name="chevron-down" size={13} />
          </button>
        )}
      </div>
      {open && (
        <Popup anchor={anchor} menuRef={menu} id={id} label="可选模型">
          {filtered.length ? (
            filtered.map((v, i) => (
              <button
                type="button"
                key={v}
                id={`${id}-${i}`}
                role="option"
                tabIndex={-1}
                aria-selected={v === value}
                data-index={i}
                data-active={active === i || undefined}
                onMouseDown={(e) => e.preventDefault()}
                onMouseEnter={() => setActive(i)}
                onClick={() => choose(v)}
              >
                <span>{v}</span>
              </button>
            ))
          ) : (
            <p className="choice-empty" role="status">
              没有匹配的模型，可继续手动输入
            </p>
          )}
        </Popup>
      )}
    </div>
  );
}

export function Help({ text }: { text: string }) {
  const anchor = useRef<HTMLSpanElement>(null),
    menu = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false),
    id = useId();
  return (
    <>
      <span
        ref={anchor}
        className="field-help"
        tabIndex={0}
        aria-label={text}
        aria-describedby={open ? id : undefined}
        onMouseEnter={() => setOpen(true)}
        onMouseLeave={() => setOpen(false)}
        onFocus={() => setOpen(true)}
        onBlur={() => setOpen(false)}
        onClick={(e) => {
          e.preventDefault();
          setOpen((v) => !v);
        }}
        onKeyDown={(e) => {
          if (e.key === "Escape" && open) {
            e.preventDefault();
            e.stopPropagation();
            setOpen(false);
          }
        }}
      >
        <Icon name="info" size={14} />
      </span>
      {open && (
        <Popup anchor={anchor} menuRef={menu} id={id} tooltip>
          {text}
        </Popup>
      )}
    </>
  );
}
