import "@fontsource-variable/inter";
import "@fontsource-variable/literata/opsz.css";
import "@fontsource-variable/literata/opsz-italic.css";
// tokens.css declares the cascade layer order, so it comes first.
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/chrome.css";
import "./styles/overlays.css";
import "./styles/settings.css";
import "./styles/article.css";
import "./styles/syntax.css";
import "./styles/state.css";
import { render } from "solid-js/web";
import { App } from "./App";
import { isMac } from "./keys";
import { start } from "./state/settings";

const root = document.querySelector("#root");
if (root === null) {
  throw new Error("index.html has no #root element");
}
document.documentElement.setAttribute("data-platform", isMac() ? "mac" : "other");
render(() => <App />, root);
void start();
