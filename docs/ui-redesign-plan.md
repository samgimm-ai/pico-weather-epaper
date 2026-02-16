# e-Paper 디스플레이 UI 리디자인 계획

## Context
현재 e-Paper 날씨 디스플레이는 텍스트만 단순 나열하는 레이아웃. 사용자가 제공한 디자인 목업(`docs/` 폴더 이미지)에 맞춰 2패널 레이아웃 + 한글 지원 + 추가 날씨 정보(습도, 풍속)를 구현한다.

## 디자인 목표 (296x128 px)

```
┌─────────────────────┬─────────────────────────┐
│  2026.02.13 (금)    │     ☀     8°C           │
│                     │         맑음             │
│  02:00:28           │                          │
│                     │   습도 45%  → 2.5m/s     │
│  Seoul              │                          │
└─────────────────────┴─────────────────────────┘
 왼쪽 패널(~148px)     오른쪽 패널(~148px)
```

## 수정할 파일

### 1. `src/ntp.rs` — DateTime에 second 필드 추가
- `DateTime` 구조체에 `pub second: u8` 추가
- `unix_to_datetime()`에서 초 계산 추가: `let second = (time_of_day % 60) as u8;`

### 2. `src/weather.rs` — 습도, 풍속 파싱 추가
- `WeatherData`에 `pub humidity: u8`, `pub wind_speed_10x: u16` 추가
  - wind_speed를 10배 정수로 저장 (f32 포맷팅 회피, 예: 2.5 → 25)
- `MainEntry`에 `humidity: u8` 추가
- `WindEntry { speed: f32 }` 구조체 추가
- `ApiResponse`에 `wind: WindEntry` 추가

### 3. `src/korean_font.rs` (신규) — 한글 비트맵 폰트
Python 스크립트(`scripts/gen_korean_font.py`)로 macOS 내장 한글 폰트에서 16x16 비트맵 생성 후 Rust const 배열로 출력.

**필요한 한글 글자 (26자):**
- 요일: 월, 화, 수, 목, 금, 토, 일 (7자)
- 날씨: 맑, 음, 흐, 림, 구, 름, 비, 눈, 안, 개, 박, 무, 연, 뇌, 우 (15자)
- 라벨: 습, 도 (2자)
- 기타: 이, 슬 (2자 - "이슬비" 표기용)

**메모리 사용**: 26자 × 32바이트 = 832바이트 (Flash에 const로 저장)

**구조:**
```rust
const GLYPH_WIDTH: u32 = 16;
const GLYPH_HEIGHT: u32 = 16;

struct KoreanGlyph {
    ch: char,
    bitmap: [u8; 32], // 16x16, 1bpp, 2 bytes/row
}

const GLYPHS: &[KoreanGlyph] = &[ ... ];

// 한글 문자열 렌더링 (한글+ASCII 혼합 지원)
pub fn draw_korean_text<D>(display: &mut D, text: &str, pos: Point, ascii_font: &MonoFont);
```

**날씨 영한 매핑 함수:**
```rust
pub fn weather_to_korean(main: &str) -> &str {
    match main {
        "Clear" => "맑음", "Clouds" => "흐림", "Rain" => "비",
        "Snow" => "눈", "Mist" => "박무", "Fog" => "안개",
        "Haze" => "연무", "Drizzle" => "이슬비", "Thunderstorm" => "뇌우",
        _ => main, // 매핑 없으면 영문 그대로
    }
}
```

### 4. `scripts/gen_korean_font.py` (신규) — 폰트 생성 스크립트
- Pillow로 macOS 내장 "Apple SD Gothic Neo" 폰트에서 16x16 렌더링
- Rust `const` 배열 코드로 출력 → `src/korean_font.rs`에 붙여넣기

### 5. `src/icons.rs` — 아이콘 크기 조정
- 기존 아이콘들을 32x32 기준으로 유지 (현재와 동일)
- 풍속 화살표 아이콘 함수 추가: `draw_wind_arrow()`
- degree 기호 그리기 함수 추가: `draw_degree_symbol()` (4px 원)

### 6. `src/display.rs` — 레이아웃 전면 재설계

**render() 함수 재작성:**

```
좌표 설계 (296x128):

왼쪽 패널 (x: 8~140):
├── 날짜: "2026.02.13 (금)"  PROFONT_12_POINT  y=18
├── 시간: "02:00:28"          PROFONT_24_POINT  y=68 (중앙)
└── 도시: "Seoul"             PROFONT_12_POINT  y=118

구분선: x=148, y=8 ~ y=120 (세로선, 1px)

오른쪽 패널 (x: 156~288):
├── 아이콘: 32x32              위치(158, 12)
├── 온도: "8°C"               PROFONT_24_POINT  y=38 (아이콘 오른쪽)
├── 날씨: "맑음"              한글 16x16         y=60 (아이콘 아래)
└── 습도/풍속: "습도 45% →2.5m/s"  PROFONT_10_POINT+한글  y=110
```

**구분선:** `Line::new(Point::new(148, 8), Point::new(148, 120))` stroke 1px

**도 기호(°):** temperature 텍스트 뒤에 4px 원 그리기

**날짜 포맷 변경:**
- 기존: `"2026.02.13 Fri"`
- 변경: `"2026.02.13 (금)"` — 괄호 안에 한글 요일

### 7. `src/main.rs` — 모듈 추가
- `mod korean_font;` 추가

## 구현 순서

1. Python 스크립트로 한글 비트맵 생성 → `korean_font.rs` 작성
2. `ntp.rs` — second 필드 추가
3. `weather.rs` — humidity, wind_speed 파싱 추가
4. `icons.rs` — degree_symbol, wind_arrow 추가
5. `display.rs` — 2패널 레이아웃 재작성
6. `main.rs` — mod 추가
7. `cargo build --release` 확인

## 검증

1. `cargo build --release` — 컴파일 성공
2. UF2 플래싱 후 e-Paper에서 확인:
   - 왼쪽: 날짜(한글요일) + 시간 + 도시
   - 오른쪽: 아이콘 + 온도(°C) + 한글날씨설명 + 습도/풍속
   - 중앙 세로 구분선
3. defmt 로그로 API 파싱 데이터 확인 (humidity, wind_speed 포함)
