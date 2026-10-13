// 확장 프로그램 빌드: src/*.ts → build/ (단일 파일들) + static/* 복사.
// content script 는 모듈을 쓸 수 없으므로 진입점마다 하나의 IIFE 로 묶는다.
import { build } from "esbuild";
import { cpSync, mkdirSync, rmSync } from "node:fs";

rmSync("build", { recursive: true, force: true });
mkdirSync("build", { recursive: true });
await build({
  entryPoints: { extract: "src/inject.ts", popup: "src/popup.ts" },
  outdir: "build",
  bundle: true,
  format: "iife",
  target: "chrome120",
  legalComments: "none",
});
cpSync("static", "build", { recursive: true });
console.log("build/ 준비 완료 — chrome://extensions 에서 '압축해제된 확장 프로그램 로드'로 이 폴더를 고르세요.");
