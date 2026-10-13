import assert from "node:assert/strict";
import { test } from "node:test";
import { JSDOM } from "jsdom";

import { adapterFor, extractConversation } from "../src/extract";
import { fileName, toMarkdown } from "../src/format";
import { htmlToMarkdown } from "../src/markdown";

function md(html: string): string {
  const dom = new JSDOM(`<div id="r">${html}</div>`);
  return htmlToMarkdown(dom.window.document.getElementById("r")!);
}

test("문단·강조·링크·인라인 코드", () => {
  assert.equal(
    md(`<p>이건 <strong>중요</strong>하고 <em>기울임</em>, <code>cargo test</code> 를 <a href="https://example.com/a">여기</a>서.</p>`),
    "이건 **중요**하고 *기울임*, `cargo test` 를 [여기](https://example.com/a)서.",
  );
});

test("본문의 Markdown 문자는 이스케이프한다", () => {
  assert.equal(md(`<p>2 * 3 = 6, [대괄호], snake_case</p>`), "2 \\* 3 = 6, \\[대괄호\\], snake\\_case");
});

test("실행되는 링크 주소는 노트로 옮기지 않는다", () => {
  assert.equal(md(`<p><a href="javascript:alert(1)">눌러</a></p>`), "눌러");
});

test("코드 블록은 언어와 내용을 그대로 유지하고, 버튼 글자는 버린다", () => {
  const out = md(
    `<pre><div><span>rust</span><button>복사</button></div><code class="language-rust">fn main() {\n    println!("a * b");\n}\n</code></pre>`,
  );
  assert.equal(out, '```rust\nfn main() {\n    println!("a * b");\n}\n```');
});

test("코드 안에 ``` 가 있어도 울타리가 깨지지 않는다", () => {
  const out = md("<pre><code>```\ninner\n```</code></pre>");
  assert.ok(out.startsWith("````\n"), out);
  assert.ok(out.endsWith("\n````"), out);
});

test("중첩 목록과 번호 목록", () => {
  assert.equal(
    md(`<ol><li>첫째<ul><li>하위 <strong>굵게</strong></li><li>하위2</li></ul></li><li><p>둘째</p></li></ol>`),
    "1. 첫째\n  - 하위 **굵게**\n  - 하위2\n2. 둘째",
  );
});

test("표", () => {
  assert.equal(
    md(`<table><thead><tr><th>항목</th><th>값</th></tr></thead><tbody><tr><td>a|b</td><td>1</td></tr></tbody></table>`),
    "| 항목 | 값 |\n| --- | --- |\n| a\\|b | 1 |",
  );
});

test("답변 안의 제목은 ### 부터 — 문서의 화자 제목(##)과 겹치지 않는다", () => {
  assert.equal(md(`<h1>큰 제목</h1><h3>작은 제목</h3><p>본문</p>`), "### 큰 제목\n\n##### 작은 제목\n\n본문");
});

test("인용과 구분선", () => {
  assert.equal(md(`<blockquote><p>한 줄</p><p>두 줄</p></blockquote><hr><p>끝</p>`), "> 한 줄\n>\n> 두 줄\n\n---\n\n끝");
});

test("모르는 태그는 내용을 잃지 않는다", () => {
  assert.equal(md(`<section><custom-thing>안의 <b>글자</b></custom-thing></section>`), "안의 **글자**");
});

// ChatGPT 화면의 모양을 흉내 낸 fixture. 실제 사이트는 바뀔 수 있다.
const CHATGPT = `<!doctype html><html><head><title>CPU 연구 - ChatGPT</title></head><body>
<article><div data-message-author-role="user"><div class="whitespace-pre-wrap">CPU 가 왜 느려?
두 번째 줄 *별표*</div></div></article>
<article><div data-message-author-role="assistant" data-message-model-slug="gpt-5">
  <div class="markdown"><p>확인할 것은 <strong>두 가지</strong>입니다.</p><ul><li>온도</li><li>전원 설정</li></ul></div>
  <div><button>복사</button><button>좋아요</button></div>
</div></article>
<article><div data-message-author-role="tool"><div>도구 출력</div></div></article>
<article><div data-message-author-role="assistant"><div class="markdown"><p>모델 표시 없는 답</p></div></div></article>
</body></html>`;

test("ChatGPT 대화 추출: 화자·모델·순서, 도구 메시지와 버튼 제외", () => {
  const dom = new JSDOM(CHATGPT);
  const r = extractConversation(dom.window.document, "https://chatgpt.com/c/abc");
  assert.ok(r.ok);
  const c = r.conversation;
  assert.equal(c.title, "CPU 연구");
  assert.equal(c.site, "ChatGPT");
  assert.equal(c.messages.length, 3, "tool 메시지는 대화가 아니다");
  assert.deepEqual(c.messages[0], { role: "user", markdown: "CPU 가 왜 느려?\n두 번째 줄 *별표*" });
  assert.deepEqual(c.messages[1], {
    role: "assistant",
    model: "gpt-5",
    markdown: "확인할 것은 **두 가지**입니다.\n\n- 온도\n- 전원 설정",
  });
  assert.equal(c.messages[2]?.model, undefined, "모델을 추측하지 않는다");
  assert.ok(!JSON.stringify(c).includes("복사"));
});

test("지원하지 않는 사이트와 빈 화면은 이유를 돌려준다 — 빈 파일을 만들지 않는다", () => {
  const dom = new JSDOM(CHATGPT);
  const other = extractConversation(dom.window.document, "https://example.com/");
  assert.ok(!other.ok);
  assert.equal(other.reason, "unsupported_site");

  const empty = new JSDOM("<html><head><title>ChatGPT</title></head><body><main></main></body></html>");
  const none = extractConversation(empty.window.document, "https://chatgpt.com/");
  assert.ok(!none.ok);
  assert.equal(none.reason, "no_messages");
  assert.match(none.detail, /화면 구조가 바뀐/);
});

test("호스트 판정은 비슷한 이름에 속지 않는다", () => {
  assert.ok(adapterFor("chatgpt.com"));
  assert.ok(adapterFor("www.chatgpt.com"));
  assert.equal(adapterFor("chatgpt.com.evil.example"), undefined);
  assert.equal(adapterFor("notchatgpt.com"), undefined);
});

test("문서 형식: front matter·화자·모델 표시", () => {
  const dom = new JSDOM(CHATGPT);
  const r = extractConversation(dom.window.document, "https://chatgpt.com/c/abc");
  assert.ok(r.ok);
  const out = toMarkdown({ ...r.conversation, title: 'CPU "연구"\n둘째 줄' }, new Date("2026-10-13T01:02:03.456Z"));
  assert.ok(out.startsWith("---\n"));
  assert.match(out, /^title: "CPU '연구' 둘째 줄"$/m);
  assert.match(out, /^exported: 2026-10-13T01:02:03Z$/m);
  assert.match(out, /^messages: 3$/m);
  assert.match(out, /^models: \["gpt-5"\]$/m);
  assert.match(out, /^## 나$/m);
  assert.match(out, /^## AI — ChatGPT \/ gpt-5$/m);
  assert.match(out, /^## AI — ChatGPT$/m);
  assert.match(out, /원문/);
});

test("파일 이름은 Windows 에서 쓸 수 있어야 한다", () => {
  const d = new Date("2026-10-13T00:00:00Z");
  assert.equal(fileName('a/b:c*d?"e"<f>|g', d), "a b c d e f g (2026-10-13).md");
  assert.equal(fileName("   ...   ", d), "대화 (2026-10-13).md");
  assert.ok(fileName("가".repeat(300), d).length < 100);
});
