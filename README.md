<div align="center">

<!-- Banner Header SVG with Kawaii styling, drop-shadows, and custom gradients -->
<svg width="100%" height="140" viewBox="0 0 800 140" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="kawaiiGrad" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" style="stop-color:#ff9a9e;stop-opacity:1" />
      <stop offset="99%" style="stop-color:#fecfef;stop-opacity:1" />
    </linearGradient>
    <filter id="shadow" x="-5%" y="-5%" width="110%" height="110%">
      <feDropShadow dx="0" dy="4" stdDeviation="4" flood-color="#ffb6c1" flood-opacity="0.5"/>
    </filter>
  </defs>
  <rect width="100%" height="100%" rx="24" fill="url(#kawaiiGrad)" filter="url(#shadow)"/>
  <text x="50%" y="55" font-family="'Comic Sans MS', 'Chalkboard SE', 'Fredoka', cursive, sans-serif" font-size="38" font-weight="bold" fill="#ffffff" text-anchor="middle">
    ✿ soyabean ✿
  </text>
  <text x="50%" y="95" font-family="'Comic Sans MS', 'Chalkboard SE', sans-serif" font-size="16" fill="#fff5f8" text-anchor="middle">
    ( ≖‿≖) a minimal, ultra-cute terminal &amp; GUI code IDE in Rust!
  </text>
</svg>

<br><br>

<!-- Kawaii Styled SVG Badges -->
<a href="https://github.com/zenformality/soyabean/releases/latest">
  <svg width="110" height="30" xmlns="http://www.w3.org/2000/svg">
    <rect width="100%" height="100%" rx="15" fill="#ff75a0"/>
    <text x="50%" y="20" font-family="sans-serif" font-size="12" font-weight="bold" fill="white" text-anchor="middle">(◕‿◕) v1.0.1</text>
  </svg>
</a>
<svg width="120" height="30" xmlns="http://www.w3.org/2000/svg">
  <rect width="100%" height="100%" rx="15" fill="#b5b2ff"/>
  <text x="50%" y="20" font-family="sans-serif" font-size="12" font-weight="bold" fill="white" text-anchor="middle">( ≖‿≖) MIT License</text>
</svg>
<svg width="90" height="30" xmlns="http://www.w3.org/2000/svg">
  <rect width="100%" height="100%" rx="15" fill="#ffb3c6"/>
  <text x="50%" y="20" font-family="sans-serif" font-size="12" font-weight="bold" fill="white" text-anchor="middle">( ◕‿◕) Rust</text>
</svg>
<svg width="170" height="30" xmlns="http://www.w3.org/2000/svg">
  <rect width="100%" height="100%" rx="15" fill="#a8e6cf"/>
  <text x="50%" y="20" font-family="sans-serif" font-size="12" font-weight="bold" fill="#2d5a44" text-anchor="middle">(๑❛ᴗ❛๑) Win · Linux · macOS</text>
</svg>

</div>

<br>

<!-- Kawaii Divider -->
<svg width="100%" height="20" xmlns="http://www.w3.org/2000/svg">
  <path d="M 0 10 Q 20 0, 40 10 T 80 10 T 120 10 T 160 10 T 200 10 T 240 10 T 280 10 T 320 10 T 360 10 T 400 10 T 440 10 T 480 10 T 520 10 T 560 10 T 600 10 T 640 10 T 680 10 T 720 10 T 760 10 T 800 10" fill="none" stroke="#ffb6c1" stroke-width="3"/>
</svg>

<br>

## 🌸 Quick Start ٩(◕‿◕｡)۶

### Installation

| Platform | Method |
|----------|--------|
| **Windows** | Download `soyabean-1.0.1-x64-setup.exe` from [Releases](https://github.com/zenformality/soyabean/releases/latest) and run the installer |
| **Linux** | Download `soyabean-1.0.1-x86_64.AppImage`, make executable, and run |
| **macOS** | Build from source (see below) |
| **From Source** | `cargo install --git https://github.com/zenformality/soyabean` |

### Run ( ≖‿≖)

```bash
# TUI version
soyabean [FILE...]

# GUI version (modern IDE with file tree, terminal, themes)
soyabean_gui [FILE...]
