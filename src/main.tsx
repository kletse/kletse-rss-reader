import "@fontsource-variable/inter";
import "./styles.css";
import { render } from "solid-js/web";
import { App } from "./App";
import { isMac } from "./keys";
import { start } from "./state";

if (isMac()) document.documentElement.classList.add("mac");
render(() => <App />, document.getElementById("root")!);
void start();
