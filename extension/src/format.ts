// 읽어 낸 대화 → Markdown 문서.
//
// APEX Velox 의 `velox chorus chat export` (velox-core::conversation::to_markdown) 와
// 같은 모양을 쓴다: YAML front matter + 제목 + 화자별 섹션. 두 곳에서 내보낸 노트가
// 같은 노트 폴더에 섞여도 같은 방식으로 읽힌다.

import type { Conversation } from "./extract";

function yaml(value: string): string {
  // 따옴표·줄바꿈이 front matter 를 깨뜨리지 않게.
  return value.replace(/"/g, "'").replace(/[\r\n]+/g, " ");
}

export function toMarkdown(c: Conversation, exportedAt: Date): string {
  const models = [...new Set(c.messages.map((m) => m.model).filter((m): m is string => Boolean(m)))];
  const lines = [
    "---",
    `title: "${yaml(c.title)}"`,
    `exported: ${exportedAt.toISOString().replace(/\.\d{3}Z$/, "Z")}`,
    `messages: ${c.messages.length}`,
    `models: [${models.map((m) => `"${yaml(m)}"`).join(", ")}]`,
    `site: "${yaml(c.site)}"`,
    `url: "${yaml(c.url)}"`,
    "source: APEX Chat Export",
    "---",
    "",
    `# ${c.title.replace(/[\r\n]+/g, " ")}`,
    "",
    "> AI 가 생성한 답이 포함된 대화 기록입니다. 확정된 결정이 아니라 **원문**입니다.",
    "",
  ];
  for (const m of c.messages) {
    const who = m.role === "user" ? "나" : `AI — ${c.site}${m.model ? ` / ${m.model}` : ""}`;
    lines.push(`## ${who}`, "", m.markdown, "");
  }
  return lines.join("\n");
}

/** 파일 이름으로 쓸 수 없는 글자를 없앤다. Windows 기준이 가장 엄격하다. */
export function fileName(title: string, exportedAt: Date): string {
  const safe = title
    .replace(/[\\/:*?"<>|\u0000-\u001f]/g, " ")
    .replace(/\s+/g, " ")
    .replace(/^[. ]+|[. ]+$/g, "")
    .slice(0, 80)
    .trim();
  const date = exportedAt.toISOString().slice(0, 10);
  return `${safe || "대화"} (${date}).md`;
}
