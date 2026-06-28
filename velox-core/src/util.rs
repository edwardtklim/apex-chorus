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
