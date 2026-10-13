// 화면에서 대화를 읽어 낸다. 사이트마다 어댑터 하나.
//
// 원칙:
// - **읽기만 한다.** 페이지를 바꾸지 않고, 입력하지 않고, 네트워크를 쓰지 않는다.
// - **사용자가 버튼을 눌렀을 때만** 실행된다(자동 수집 없음).
// - 못 읽었으면 빈 결과를 지어내지 않고 이유를 돌려준다. 사이트가 화면 구조를 바꾸면
//   조용히 빈 파일을 만드는 대신 "읽지 못했다"고 말해야 한다.

import { htmlToMarkdown } from "./markdown";

export type Role = "user" | "assistant";

export interface Message {
  role: Role;
  /** 답한 모델(화면에 표시된 경우에만). 추측하지 않는다. */
  model?: string;
  markdown: string;
}

export interface Conversation {
  site: string;
  title: string;
  url: string;
  messages: Message[];
}

export type ExtractResult =
  | { ok: true; conversation: Conversation }
  | { ok: false; reason: "unsupported_site" | "no_messages"; detail: string };

interface Adapter {
  site: string;
  hosts: string[];
  extract(doc: Document): Message[];
  title(doc: Document): string;
}

const chatgpt: Adapter = {
  site: "ChatGPT",
  hosts: ["chatgpt.com", "chat.openai.com"],
  extract(doc) {
    // ChatGPT 는 메시지마다 화자를 속성으로 표시한다. 클래스 이름보다 훨씬 덜 바뀐다.
    const out: Message[] = [];
    for (const el of Array.from(doc.querySelectorAll("[data-message-author-role]"))) {
      const role = el.getAttribute("data-message-author-role");
      if (role !== "user" && role !== "assistant") continue; // system·tool 은 대화 본문이 아니다
      // 답변은 렌더링된 Markdown 영역이 있으면 그것만 읽는다(버튼·출처 칩 제외).
      const body = (role === "assistant" && el.querySelector(".markdown")) || el;
      const markdown =
        role === "user"
          ? // 사용자가 친 글은 줄바꿈이 의미다. 서식 변환 없이 글자 그대로 둔다.
            (body.textContent ?? "").replace(/\r/g, "").trim()
          : htmlToMarkdown(body);
      if (!markdown) continue;
      const model = el.getAttribute("data-message-model-slug") ?? undefined;
      out.push(role === "assistant" && model ? { role, model, markdown } : { role, markdown });
    }
    return out;
  },
  title(doc) {
    return doc.title.replace(/\s*[-|]\s*ChatGPT\s*$/i, "").trim();
  },
};

export const ADAPTERS: Adapter[] = [chatgpt];

export function adapterFor(hostname: string): Adapter | undefined {
  const host = hostname.toLowerCase();
  return ADAPTERS.find((a) => a.hosts.some((h) => host === h || host.endsWith(`.${h}`)));
}

export function supportedSites(): string {
  return ADAPTERS.map((a) => a.site).join(", ");
}

export function extractConversation(doc: Document, url: string): ExtractResult {
  let hostname = "";
  try {
    hostname = new URL(url).hostname;
  } catch {
    /* 주소를 못 읽으면 지원하지 않는 사이트로 처리한다 */
  }
  const adapter = adapterFor(hostname);
  if (!adapter) {
    return {
      ok: false,
      reason: "unsupported_site",
      detail: `이 사이트는 아직 지원하지 않습니다. 지원: ${supportedSites()}`,
    };
  }
  const messages = adapter.extract(doc);
  if (messages.length === 0) {
    return {
      ok: false,
      reason: "no_messages",
      detail: `${adapter.site} 화면에서 대화를 찾지 못했습니다. 대화를 연 상태인지 확인하세요. 계속 안 되면 사이트의 화면 구조가 바뀐 것일 수 있습니다.`,
    };
  }
  return {
    ok: true,
    conversation: { site: adapter.site, title: adapter.title(doc) || "제목 없는 대화", url, messages },
  };
}
