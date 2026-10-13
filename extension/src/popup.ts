// 팝업: 버튼 하나. 누르면 지금 탭의 대화를 읽어 Markdown 파일로 저장한다.
//
// 권한은 activeTab + scripting 뿐이다. 사이트 접근 권한을 미리 받지 않고, 사용자가
// 이 버튼을 누른 탭에만 그 순간 접근한다. 대화 내용은 어디로도 전송하지 않는다.

import { adapterFor, supportedSites } from "./extract";
import type { ExportPayload } from "./inject";

const button = document.getElementById("export") as HTMLButtonElement;
const status = document.getElementById("status") as HTMLParagraphElement;

function say(text: string, kind: "info" | "ok" | "error" = "info"): void {
  status.textContent = text;
  status.dataset.kind = kind;
}

function download(name: string, text: string): void {
  // BOM 을 붙여 Windows 메모장에서도 한글이 깨지지 않게 한다.
  const url = URL.createObjectURL(new Blob(["﻿", text], { type: "text/markdown;charset=utf-8" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10_000);
}

async function activeTab(): Promise<chrome.tabs.Tab | undefined> {
  const [tab] = await chrome.tabs.query({ active: true, currentWindow: true });
  return tab;
}

async function init(): Promise<void> {
  const tab = await activeTab();
  let host = "";
  try {
    host = new URL(tab?.url ?? "").hostname;
  } catch {
    /* 주소 없는 탭 */
  }
  if (!adapterFor(host)) {
    button.disabled = true;
    say(`이 페이지는 지원하지 않습니다. 지원: ${supportedSites()}`);
  }
}

button.addEventListener("click", async () => {
  button.disabled = true;
  say("대화를 읽는 중...");
  try {
    const tab = await activeTab();
    if (tab?.id === undefined) throw new Error("현재 탭을 찾을 수 없습니다.");
    await chrome.scripting.executeScript({ target: { tabId: tab.id }, files: ["extract.js"] });
    const [res] = await chrome.scripting.executeScript({
      target: { tabId: tab.id },
      func: () => (globalThis as unknown as { __apexChatExport: () => unknown }).__apexChatExport(),
    });
    const payload = res?.result as ExportPayload | undefined;
    if (!payload) throw new Error("페이지에서 응답이 없습니다. 페이지를 새로고침한 뒤 다시 시도하세요.");
    if (!payload.ok) {
      say(payload.detail, "error");
      return;
    }
    download(payload.fileName, payload.markdown);
    say(`저장했습니다 · 메시지 ${payload.messages}개 · ${payload.fileName}`, "ok");
  } catch (e) {
    say(e instanceof Error ? e.message : String(e), "error");
  } finally {
    button.disabled = false;
  }
});

void init();
