// 콘솔/명령 출력 디코딩.
//
// powercfg·wevtutil 같은 Windows 내장 도구는 시스템 코드페이지(한국어 = CP949/EUC-KR)로
// 출력한다. 이를 UTF-8로 읽으면 한글이 깨지므로, UTF-8이 아니면 EUC-KR(=Windows-949)로
// 폴백 디코딩한다. (ASCII는 둘 다 동일하게 처리됨)

pub fn decode_console(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => encoding_rs::EUC_KR.decode(bytes).0.into_owned(),
    }
}

/// 현재 시각을 UTC RFC3339 문자열로. 외부 크레이트 없이 만든다.
///
/// 기록(리포트·대화·장부)의 시각 표기는 전부 이 함수를 쓴다 — 포맷이 갈리면
/// 나중에 파일을 섞어 읽을 수 없다.
pub fn now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}

/// days-since-epoch → (year, month, day). Howard Hinnant 의 civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passes_through_valid_utf8() {
        assert_eq!(decode_console("hello 안녕".as_bytes()), "hello 안녕");
        assert_eq!(decode_console(b"plain ascii"), "plain ascii");
    }

    #[test]
    fn falls_back_to_euc_kr_for_non_utf8() {
        // powercfg 한국어 출력처럼 EUC-KR(CP949)로 인코딩된 바이트.
        let euc_kr = encoding_rs::EUC_KR.encode("전원 구성표").0;
        assert!(
            std::str::from_utf8(&euc_kr).is_err(),
            "EUC-KR 바이트가 UTF-8이 아니어야 폴백 경로가 테스트됨"
        );
        assert_eq!(decode_console(&euc_kr), "전원 구성표");
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(decode_console(b""), "");
    }
}
