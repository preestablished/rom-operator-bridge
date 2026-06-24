import { mountOperatorApp } from "./app";
import { loadRuntimeConfig } from "./runtimeConfig";
import "./styles.css";

const root = document.querySelector<HTMLDivElement>("#app");

if (root) {
  loadRuntimeConfig().then((config) => {
    mountOperatorApp(root, config);
  });
}
