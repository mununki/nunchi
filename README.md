# Araso for Windows

[Araso](https://github.com/nyttastudio/araso)(아라소, macOS 입력기)를 Windows로 옮기는 프로젝트입니다.
영문(QWERTY) 자판으로 치면 단어마다 영어인지 한국어인지 알아보고 맞는 글자로 넣습니다. 한/영 키를 누를 일이 없습니다.

```
Stay hungry, stay foolishsms wkqtmdml akfdlek.  →  Stay hungry, stay foolish는 잡스의 말이다.
dlrjs githubdp commitgkf rjdi.                   →  이건 github에 commit할 거야.
PDFvkdlf hellodnjfem                             →  PDF파일 hello월드
```

판단 로직은 원본 ArasoCore(Swift)를 Rust로 그대로 옮겼고, 입력기 껍데기는 Windows의 TSF(Text Services
Framework)로 새로 만들었습니다. 옮긴 기준은 원본 커밋 [22e7d3d](https://github.com/nyttastudio/araso/commit/22e7d3d)이고,
단어 목록과 모델도 그때의 것을 `data/`에 그대로 복사했고, 빌드할 때 표로 바꿔 DLL에 넣습니다. 입력기는 모든 앱 안에서
돌기 때문에, 앱마다 목록을 읽어 올리는 대신 DLL의 표를 함께 씁니다(앱 하나당 추가 메모리 약 1~2 MB, 키 하나 판단 0.04 ms 이하).

> **비공식 포트입니다.** 원본 Araso 개발자(nyttastudio)와 관계없이 만들고 있습니다.

> 상태: **개발 중 (0.1)**. 영어·한국어는 동작하고, 설치 프로그램·설정 창·일본어 한자 변환은 아직 없습니다.

## 되는 것

- 단어마다 영어/한국어 판단(두벌식, 세벌식 최종), 앞 단어의 언어 쪽으로 기울이기
- 섞인 단어: `github에`, `x에`, `PDF파일`, `hello월드`
- 언어 바꾸기 키로 치고 있는 단어와 **방금 확정한 단어** 고치기, 같은 쪽으로 두 번 고치면 학습(`dictionary.json`)
- 한국어 Delete(치는 글자는 자모 하나씩, 그 앞은 글자째), URL·경로 안에서는 친 그대로
- 앱별 설정(`apps.json`: 끄기, 엄격하게, `holdSpace`), 일시정지 키, Colemak/Colemak-DH, vim 모드 연동(신호를 보내는 쪽은 아직)
- 일본어는 로마자 → 가나 판단까지만(기본 꺼짐, 한자 변환 없음)

## 키 (Windows 기본값)

| 하는 일 | 기본 키 | 원본(macOS) |
|---|---|---|
| 언어 바꾸기 (영어 → 영어+조사 → 한국어 …) | **한/영 키**, 오른쪽 Alt 단독, Shift+Esc | 오른쪽 ⌥ 단독, ⇧Esc |
| 이 입력창에서 일시정지 | **한자 키**(없으면 오른쪽 Ctrl 단독) | 오른쪽 ⌘ 단독 |

`settings.json`의 `pauseKey`, `apps.json`의 `switchKeys`로 바꿀 수 있습니다. 이름은 `hangulKey`, `rightAlt`,
`rightShift`, `escape`, `shiftEscape`(언어 바꾸기)와 `hanjaKey`, `rightControl`, `rightAlt`, `rightShift`, `off`(일시정지)입니다.
원본의 `rightOption`, `rightCommand`도 읽습니다.

## 설정 파일

`%APPDATA%\Araso\`에 원본과 같은 형식의 JSON이 있습니다. 앱은 실행 파일 이름(소문자)으로 씁니다.

```json
// apps.json
{ "*": { "switchKeys": ["hangulKey", "shiftEscape"] },
  "windowsterminal.exe": { "conversionThreshold": 0.97, "holdSpace": true },
  "game.exe": { "enabled": false } }
```

바꾼 내용은 2초 안에 모든 앱에 반영됩니다. `%APPDATA%\Araso\logging` 파일을 만들면 `logs\`에 진단 기록을 남깁니다
(키의 종류만, 친 글자는 남기지 않습니다).

## 빌드와 테스트

Rust(stable, MSVC)와 Visual Studio Build Tools(C++ 워크로드)가 필요합니다.

```bash
cargo test -p araso-core          # 테스트 170개 (대부분 원본 Swift 테스트를 옮긴 것)
cargo run -p araso-cli -- -v "dkssud hello githubdp"
```

## 개발용으로 설치해 보기

```bash
powershell -ExecutionPolicy Bypass -File scripts/dev-register.ps1
```

관리자 권한(UAC)을 묻고 `target\dev-install`의 사본을 등록합니다. 그다음 **설정 → 시간 및 언어 → 언어 및 지역 →
한국어 → 언어 옵션 → 키보드 추가 → Araso**로 한 번 추가하고, Win+Space로 Araso를 고르세요.

- 고친 뒤 다시 넣기: `scripts/dev-update.ps1` (UAC 없이, 새로 여는 앱부터 반영)
- 실제 키 입력 테스트: `cargo run --release -p araso-typing-test -- --richedit` (`--imm`: IMM32 앱처럼, 옵션 없이: Win32 EDIT).
  자기 창을 띄워 몇 초간 키보드를 쓰고, 포커스를 잃으면 멈춥니다.
- 제거: `scripts/dev-register.ps1 -Unregister`

## 구조

| 경로 | 내용 |
|---|---|
| `crates/araso-core` | 원본 ArasoCore를 옮긴 판단 엔진과 `InputSession`(플랫폼 코드 없음) |
| `crates/araso-tsf` | TSF 입력기 DLL: 키 → `InputSession`, 조합 문자열 ↔ TSF composition |
| `crates/araso-cli` | 원본 `araso-cli`와 같은 명령줄 도구 |
| `crates/araso-typing-test` | 진짜 키 이벤트로 입력해 보는 테스트 |
| `data/` | 단어 목록과 모델(원본에서 복사), 출처와 라이선스는 `data/README.md` |

## 알려진 한계

- **IMM32 앱**(클래식 Win32 입력란, Qt, Java, 많은 게임)은 확정한 글자를 입력기에 돌려주지 못해 *확정한 뒤에 고치기*가 안 됩니다.
  그런 앱에서는 `holdSpace`를 켜면 Space 뒤에도 단어를 잡아 둡니다(원본의 터미널 대책과 같음). 치는 중에 바꾸기는 됩니다.
- 64비트 앱만: 32비트 DLL은 아직 빌드하지 않았습니다.
- 스토어(샌드박스) 앱은 개발 설치 경로(`target\dev-install`)의 DLL을 읽지 못해 Araso를 쓸 수 없습니다. `Program Files`에 설치하는 패키지가 필요합니다.
- 설정 창, 작업 표시줄 메뉴, 업데이트 확인, 일본어 한자 변환(MS-IME 엔진 연동 예정)이 없습니다.

## 라이선스

코드는 MIT입니다(`LICENSE`, 원본의 저작권 표시 포함). `data/`의 파일은 만든 재료의 라이선스를 따르고(Tatoeba
CC BY 2.0 FR, SCOWL, FrequencyWords CC BY-SA 4.0, Webster's 2nd 퍼블릭 도메인), 자세한 내용은
`THIRD-PARTY.md`와 `data/README.md`에 있습니다.
