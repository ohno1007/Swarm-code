// Transport abstraction: works both inside Tauri (IPC) and in a plain browser
// talking to the local axum server (HTTP + NDJSON streaming).

import { invoke, Channel } from "@tauri-apps/api/core";

const IN_TAURI =
  typeof window !== "undefined" && !!window.__TAURI_INTERNALS__;

/** Call a backend command with an args object, returning the JSON result. */
export async function call(cmd, args) {
  if (IN_TAURI) return invoke(cmd, args || {});
  const res = await fetch(`/api/${cmd}`, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(args || {}),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(text || res.statusText);
  return text ? JSON.parse(text) : null;
}

/**
 * Send a message, streaming AgentMsg objects to `onMsg`. Resolves with the
 * final assistant text.
 */
export async function send(id, text, onMsg) {
  if (IN_TAURI) {
    const channel = new Channel();
    channel.onmessage = onMsg;
    return invoke("send", { id, text, onEvent: channel });
  }

  const res = await fetch("/api/send", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ id, text }),
  });
  if (!res.ok) throw new Error((await res.text()) || res.statusText);

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buf = "";
  let result = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    buf += decoder.decode(value, { stream: true });
    let nl;
    while ((nl = buf.indexOf("\n")) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      const obj = JSON.parse(line);
      if (obj.t === "msg") onMsg(obj.msg);
      else if (obj.t === "done") result = obj.result;
      else if (obj.t === "error") throw new Error(obj.error);
    }
  }
  return result;
}
