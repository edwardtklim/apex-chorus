// HTML(DOM) → Markdown.
//
// AI 답변은 화면에서 이미 렌더링된 HTML 이다. 노트 앱에서 그대로 읽히도록 Markdown 으로
// 되돌린다. 순수 함수다 — DOM 을 읽기만 하고, 네트워크·저장소·페이지 상태를 건드리지 않는다.
//
// 모르는 태그는 버리지 않고 안의 글자를 그대로 통과시킨다. 내용을 조용히 잃는 것보다
// 서식을 잃는 편이 낫다.

const ELEMENT = 1;
const TEXT = 3;

// 화면에는 있지만 대화 내용이 아닌 것(복사 버튼, 아이콘 등).
const SKIP = new Set(["BUTTON", "SVG", "STYLE", "SCRIPT", "NOSCRIPT", "TEXTAREA", "INPUT", "SELECT"]);

function escapeInline(text: string): string {
  // 본문 글자가 Markdown 문법으로 오해되지 않게. 과하게 이스케이프하면 읽기 나빠지므로 최소한만.
  return text.replace(/([\\`*_\[\]])/g, "\\$1");
}

function collapse(text: string): string {
  return text.replace(/\s+/g, " ");
}

function codeLanguage(code: Element | null, pre: Element): string {
  for (const el of [code, pre]) {
    const m = el?.getAttribute("class")?.match(/language-([\w+#.-]+)/);
    if (m?.[1]) return m[1];
  }
  return "";
}

function fence(body: string): string {
  // 코드 안에 ``` 가 있어도 깨지지 않게 더 긴 울타리를 쓴다.
  const longest = Math.max(2, ...Array.from(body.matchAll(/`+/g), (m) => m[0].length));
  return "`".repeat(longest + 1);
}

function inline(node: Node): string {
  if (node.nodeType === TEXT) return escapeInline(collapse(node.textContent ?? ""));
  if (node.nodeType !== ELEMENT) return "";
  const el = node as Element;
  const tag = el.tagName.toUpperCase();
  if (SKIP.has(tag)) return "";

  const inner = () => Array.from(el.childNodes, inline).join("");
  switch (tag) {
    case "BR":
      return "\n";
    case "STRONG":
    case "B": {
      const t = inner().trim();
      return t ? `**${t}**` : "";
    }
    case "EM":
    case "I": {
      const t = inner().trim();
      return t ? `*${t}*` : "";
    }
    case "DEL":
    case "S": {
      const t = inner().trim();
      return t ? `~~${t}~~` : "";
    }
    case "CODE": {
      const t = el.textContent ?? "";
      if (!t) return "";
      const f = t.includes("`") ? "``" : "`";
      return `${f}${t}${f}`;
    }
    case "A": {
      const t = inner().trim();
      const href = el.getAttribute("href") ?? "";
      // 실행되는 주소(javascript: 등)는 노트로 옮기지 않는다.
      if (!t) return "";
      return /^(https?:|mailto:)/i.test(href) ? `[${t}](${href})` : t;
    }
    case "IMG": {
      const alt = el.getAttribute("alt")?.trim();
      return alt ? `[이미지: ${escapeInline(alt)}]` : "[이미지]";
    }
    default:
      return inner();
  }
}

function listItems(list: Element, depth: number): string {
  const ordered = list.tagName.toUpperCase() === "OL";
  const start = Number(list.getAttribute("start") ?? "1") || 1;
  const indent = "  ".repeat(depth);
  const lines: string[] = [];
  let n = start;
  for (const li of Array.from(list.children)) {
    if (li.tagName.toUpperCase() !== "LI") continue;
    const nested: string[] = [];
    const parts: string[] = [];
    for (const child of Array.from(li.childNodes)) {
      const t = child.nodeType === ELEMENT ? (child as Element).tagName.toUpperCase() : "";
      if (t === "UL" || t === "OL") nested.push(listItems(child as Element, depth + 1));
      else if (t === "PRE") nested.push(indentBlock(block(child), depth + 1));
      else if (t === "P" || t === "DIV") parts.push(Array.from(child.childNodes, inline).join("").trim());
      else parts.push(inline(child));
    }
    const text = parts.join(" ").replace(/ +/g, " ").trim();
    lines.push(`${indent}${ordered ? `${n}.` : "-"} ${text}`.trimEnd());
    lines.push(...nested.filter(Boolean));
    n += 1;
  }
  return lines.join("\n");
}

function indentBlock(text: string, depth: number): string {
  const pad = "  ".repeat(depth);
  return text
    .split("\n")
    .map((l) => (l ? pad + l : l))
    .join("\n");
}

function table(el: Element): string {
  const rows = Array.from(el.querySelectorAll("tr")).map((tr) =>
    Array.from(tr.children, (cell) =>
      Array.from(cell.childNodes, inline).join("").replace(/\n/g, " ").replace(/\|/g, "\\|").trim(),
    ),
  );
  const head = rows[0];
  if (!head || head.length === 0) return "";
  const line = (cells: string[]) => `| ${head.map((_, i) => cells[i] ?? "").join(" | ")} |`;
  return [line(head), `| ${head.map(() => "---").join(" | ")} |`, ...rows.slice(1).map(line)].join("\n");
}

function block(node: Node): string {
  if (node.nodeType === TEXT) return escapeInline(collapse(node.textContent ?? "")).trim();
  if (node.nodeType !== ELEMENT) return "";
  const el = node as Element;
  const tag = el.tagName.toUpperCase();
  if (SKIP.has(tag)) return "";

  switch (tag) {
    case "H1":
    case "H2":
    case "H3":
    case "H4":
    case "H5":
    case "H6": {
      // 문서의 # / ## 는 제목과 화자에 쓴다. 답변 안의 제목은 ### 부터 시작한다.
      const level = Math.min(6, Number(tag[1]) + 2);
      const t = Array.from(el.childNodes, inline).join("").trim();
      return t ? `${"#".repeat(level)} ${t}` : "";
    }
    case "P":
      return Array.from(el.childNodes, inline).join("").trim();
    case "PRE": {
      const code = el.querySelector("code");
      const body = (code ?? el).textContent?.replace(/\n$/, "") ?? "";
      const f = fence(body);
      return `${f}${codeLanguage(code, el)}\n${body}\n${f}`;
    }
    case "UL":
    case "OL":
      return listItems(el, 0);
    case "BLOCKQUOTE":
      return blocks(el)
        .split("\n")
        .map((l) => (l ? `> ${l}` : ">"))
        .join("\n");
    case "TABLE":
      return table(el);
    case "HR":
      return "---";
    default: {
      // 블록 자식이 있으면 블록으로, 아니면 한 문단으로.
      const hasBlock = Array.from(el.children).some((c) =>
        /^(P|PRE|UL|OL|H[1-6]|BLOCKQUOTE|TABLE|DIV|HR|SECTION|ARTICLE)$/.test(c.tagName.toUpperCase()),
      );
      return hasBlock ? blocks(el) : Array.from(el.childNodes, inline).join("").trim();
    }
  }
}

function blocks(el: Element): string {
  return Array.from(el.childNodes, block)
    .map((b) => b.trim())
    .filter(Boolean)
    .join("\n\n");
}

/** 한 메시지의 내용 영역을 Markdown 으로. */
export function htmlToMarkdown(root: Element): string {
  return blocks(root)
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
