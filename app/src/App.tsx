import { CameraProvider } from "./lib/camera";
import { PrefsProvider } from "./lib/prefs";
import { useRoute } from "./lib/router";
import { ToastProvider } from "./lib/toast";
import { Sidebar } from "./shell/Sidebar";
import { Hub } from "./pages/Hub";
import { Recipes } from "./pages/Recipes";
import { Settings } from "./pages/Settings";
import { Lab } from "./pages/Lab";
import { Connect } from "./pages/Connect";

export function App() {
  return (
    <PrefsProvider>
      <CameraProvider>
        <ToastProvider>
          <Shell />
        </ToastProvider>
      </CameraProvider>
    </PrefsProvider>
  );
}

function Shell() {
  const { page, sub } = useRoute();
  return (
    <div className="shell">
      <Sidebar page={page} />
      <main className="main" data-page={page}>
        {page === "hub" && <Hub />}
        {page === "recipes" && <Recipes />}
        {page === "settings" && <Settings sub={sub} />}
        {page === "connect" && <Connect />}
        <Lab active={page === "lab"} />
      </main>
    </div>
  );
}
