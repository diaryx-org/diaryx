import "./app.css";
import { mount } from "svelte";

if (import.meta.env.DEV && typeof window !== "undefined") {
  const { protocol, hostname, port, pathname, search, hash } = window.location;
  const isLocalHttp = protocol === "http:" || protocol === "https:";
  if (isLocalHttp && (hostname === "127.0.0.1" || hostname === "[::1]")) {
    const target = `${protocol}//localhost${port ? `:${port}` : ""}${pathname}${search}${hash}`;
    window.location.replace(target);
  }
}

const target = document.getElementById("app");

if (target) {
  target.innerHTML = "";

  const params = new URLSearchParams(window.location.search);
  if (params.has("preview")) {
    // Lightweight preview mode — renders a themed workspace mockup
    // Used by the onboarding carousel via iframe
    import("./views/PreviewApp.svelte").then(({ default: PreviewApp }) => {
      mount(PreviewApp, {
        target,
        props: {
          bundleId: params.get("bundle") ?? "bundle.default",
          darkMode: params.get("dark") === "1",
        },
      });
    });
  } else {
    import("./App.svelte").then(({ default: App }) => {
      mount(App, { target });
    });
  }
}
