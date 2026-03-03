import "./app.css";
import { mount } from "svelte";
import App from "./App.svelte";

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
  // Clear the loading placeholder before mounting
  target.innerHTML = "";
  mount(App, { target });
}
