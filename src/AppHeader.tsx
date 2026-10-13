import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { native } from "./api";
import { Icon } from "./ui";
import patrickIcon from "./assets/patrick-icon.png";

export function AppHeader({
  version,
  onError,
}: {
  version: string;
  onError: (message: string) => void;
}) {
  const [maximized, setMaximized] = useState(false);
  useEffect(() => {
    if (!native) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    const window = getCurrentWindow();
    const update = async () => {
      try {
        const value = await window.isMaximized();
        if (!disposed) setMaximized(value);
      } catch (error) {
        if (!disposed) onError(String(error));
      }
    };
    void update();
    void window
      .onResized(() => void update())
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch((error) => {
        if (!disposed) onError(String(error));
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [onError]);
  const action = (fn: () => Promise<void>) => {
    void fn().catch((error) => onError(String(error)));
  };
  return (
    <header className="window-header">
      <div className="window-drag-layer" data-tauri-drag-region />
      <div className="window-header-ui">
        <div className="compact-brand">
          <img
            className="app-logo"
            src={patrickIcon}
            alt="DSH Patrick 图标"
            width={20}
            height={20}
          />
          <span>DSH Patrick v{version}</span>
        </div>
        <div className="window-controls">
          <button
            type="button"
            disabled={!native}
            aria-label="最小化窗口"
            title="最小化窗口"
            onClick={() => action(() => getCurrentWindow().minimize())}
          >
            <Icon name="window-minimize" size={15} />
          </button>
          <button
            type="button"
            disabled={!native}
            aria-label={maximized ? "还原窗口" : "最大化窗口"}
            title={maximized ? "还原窗口" : "最大化窗口"}
            onClick={() => action(() => getCurrentWindow().toggleMaximize())}
          >
            <Icon
              name={maximized ? "window-restore" : "window-maximize"}
              size={15}
            />
          </button>
          <button
            type="button"
            disabled={!native}
            aria-label="关闭窗口"
            title="关闭窗口"
            onClick={() => action(() => getCurrentWindow().close())}
          >
            <Icon name="window-close" size={15} />
          </button>
        </div>
      </div>
    </header>
  );
}
