# APEX Chat Export — Chrome 확장 프로그램

보고 있는 AI 대화를 Markdown 파일로 저장한다. 첫 지원 사이트는 ChatGPT.

## 써보기 (스토어 등록 전)

```bash
npm install
npm run build
```

1. Chrome 주소창에 `chrome://extensions`
2. 오른쪽 위 **개발자 모드** 켜기
3. **압축해제된 확장 프로그램을 로드합니다** → 이 폴더의 `build` 선택
4. ChatGPT 대화를 연 탭에서 확장 프로그램 아이콘 → **Markdown으로 저장**

## 원칙

- **대화는 어디로도 전송하지 않는다.** 네트워크 코드가 없다. 브라우저 안에서 읽고 파일로 저장할 뿐이다.
- **권한은 `activeTab` + `scripting` 뿐.** 사이트 접근 권한을 미리 받지 않는다. 버튼을 누른 탭에만, 그 순간에만 접근한다.
- **읽기만 한다.** 페이지를 바꾸거나 입력하지 않는다. 자동 수집이 없다.
- **못 읽으면 못 읽었다고 한다.** 사이트가 화면 구조를 바꿔 대화를 찾지 못하면 빈 파일을 만들지 않고 이유를 보여준다.
- **모델을 추측하지 않는다.** 화면에 표시된 경우에만 적는다.

## 구조

| 파일 | 역할 |
|---|---|
| `src/markdown.ts` | HTML(DOM) → Markdown. 순수 함수 |
| `src/extract.ts` | 사이트 어댑터. 사이트를 추가하려면 `ADAPTERS` 에 하나 더 |
| `src/format.ts` | 문서 형식. `velox chorus chat export` 와 같은 front matter |
| `src/inject.ts` | 페이지에 주입되어 결과를 돌려주는 함수 하나를 등록 |
| `src/popup.ts` | 버튼 → 주입 → 파일 저장 |
| `static/` | `manifest.json`, `popup.html` |

검증: `npm run check` (타입 검사 · 테스트 · 빌드)

## 알려진 한계

- 추출은 ChatGPT 의 `data-message-author-role` 속성에 의존한다. 사이트가 이를 바꾸면 고쳐야 한다. 테스트는 그 모양을 흉내 낸 fixture 로 돈다 — **실제 사이트에서의 동작은 사람이 직접 확인해야 한다.**
- 화면에 로드된 메시지만 읽는다. 아주 긴 대화에서 위쪽이 아직 로드되지 않았으면 빠질 수 있다.
- 이미지·파일 첨부는 `[이미지]` 표시만 남긴다.
- 스토어용 아이콘이 아직 없다.
