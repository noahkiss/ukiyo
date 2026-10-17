import { createContext, useCallback, useContext, useState, type ReactNode } from "react";

const ToastContext = createContext<(msg: string) => void>(() => {});

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toast, setToast] = useState<string | null>(null);
  const flash = useCallback((msg: string) => {
    setToast(msg);
    window.setTimeout(() => setToast((t) => (t === msg ? null : t)), 4000);
  }, []);
  return (
    <ToastContext.Provider value={flash}>
      {children}
      {toast && (
        <div className="toast" role="status" data-component="toast">
          {toast}
        </div>
      )}
    </ToastContext.Provider>
  );
}

export const useToast = () => useContext(ToastContext);

/** For a control whose backend is not built yet. */
export function useNotBuilt() {
  const flash = useToast();
  return useCallback((what: string) => flash(`${what} is not built yet.`), [flash]);
}
