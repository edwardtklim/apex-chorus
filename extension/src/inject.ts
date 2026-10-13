// 페이지에 주입되는 스크립트. 사용자가 팝업의 버튼을 눌렀을 때만 주입된다.
// 화면을 읽어 결과를 돌려줄 함수 하나를 등록할 뿐, 스스로는 아무것도 하지 않는다.

import { extractConversation, type ExtractResult } from "./extract";
import { fileName, toMarkdown } from "./format";

export type ExportPayload =
  | { ok: true; markdown: string; fileName: string; messages: number; title: string; site: string }
  | { ok: false; detail: string };

function run(): ExportPayload {
  const result: ExtractResult = extractConversation(document, location.href);
  if (!result.ok) return { ok: false, detail: result.detail };
  const now = new Date();
  const c = result.conversation;
  return {
    ok: true,
    markdown: toMarkdown(c, now),
    fileName: fileName(c.title, now),
    messages: c.messages.length,
    title: c.title,
    site: c.site,
  };
}

(globalThis as unknown as { __apexChatExport: () => ExportPayload }).__apexChatExport = run;
