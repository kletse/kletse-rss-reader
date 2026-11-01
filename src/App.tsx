// Kletse RSS: one unread article at a time, oldest first. Or, in history mode, read ones, newest first.
import { type Accessor, createEffect, type JSX, Match, on, onCleanup, onMount, Show, Switch } from "solid-js";
import type { Entry } from "./api";
import { Article } from "./components/Article";
import { Confirm, Help, Toast } from "./components/Overlays";
import { Picker } from "./components/Picker";
import { Settings } from "./components/Settings";
import { TopBar } from "./components/TopBar";
import { onKey } from "./keyboard";
import { checking, current, hasMore, index, loading, openLink, queue, queueMode } from "./state/queue";
import { inSettings, overlay, scrollToTop, setReader } from "./state/ui";
import { followWindowTitle } from "./title";

/** Links (in articles and titles) always open in the browser. */
function onClick(e: MouseEvent): void {
  const link = e.target instanceof Element ? e.target.closest("a[href]") : undefined;
  if (link instanceof HTMLAnchorElement) {
    e.preventDefault();
    void openLink(link.href);
  }
}

function Message(props: { title?: string; text: string }): JSX.Element {
  return (
    <div class="message">
      <Show when={props.title}>{(title: Accessor<string>): JSX.Element => <h1>{title()}</h1>}</Show>
      <p>{props.text}</p>
    </div>
  );
}

function Reader(): JSX.Element {
  return (
    <Switch>
      <Match when={inSettings()}>
        <Settings firstRun={overlay() === "onboarding"} />
      </Match>
      <Match when={loading()}>
        <Message text="Loading…" />
      </Match>
      <Match when={current()} keyed={true}>
        {(entry: Entry): JSX.Element => <Article entry={entry} />}
      </Match>
      <Match when={hasMore()}>
        <Message text="Loading…" />
      </Match>
      {/* While it looks for new arrivals, nothing: no "Loading…" if there are none, no "All caught up" if there are. */}
      <Match when={queue().length > 0 && !checking()}>
        <Message
          title={queueMode() === "history" ? "End of history" : "All caught up"}
          text="Press j or Enter for the feeds, k to go back."
        />
      </Match>
    </Switch>
  );
}

export function App(): JSX.Element {
  onMount(() => {
    globalThis.addEventListener("keydown", onKey);
    document.addEventListener("click", onClick);
  });
  onCleanup(() => {
    globalThis.removeEventListener("keydown", onKey);
    document.removeEventListener("click", onClick);
  });
  // A new article (or the settings form) starts at the top.
  createEffect(on([queue, index, inSettings], scrollToTop));
  followWindowTitle();

  return (
    <>
      <TopBar />
      <main id="reader" tabindex="-1" ref={setReader}>
        <Reader />
      </main>
      <Show when={overlay() === "picker"}>
        <Picker />
      </Show>
      <Show when={overlay() === "help"}>
        <Help />
      </Show>
      <Show when={overlay() === "confirm"}>
        <Confirm />
      </Show>
      <Toast />
    </>
  );
}
