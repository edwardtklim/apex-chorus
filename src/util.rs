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
