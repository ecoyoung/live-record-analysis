# Iteration log — 直播录制分析

Compare against 爱复盘 desktop: keep probe / auto-record / 10-way ingest / ASR / compass+qianchuan / danmaku. Drop Chromium UI, SaaS login, cloud.

## Direction

1. **CPU** — no Cef; ffmpeg copy-codec; warm Node signer; skip re-probe while recording; prefer HD1 over FULL_HD1; batch SQLite danmaku.
2. **Stability** — cap 10 recorders; quality fallback; ffmpeg reconnect + watchdog; WSS signature cache; WAL.
3. **Product loop** — live → record + danmaku → collect dashboards → stop → ASR/LLM.

## Key points vs 爱复盘

| | 爱复盘 | This tree |
|---|---|---|
| UI | CefSharp Chrome | none |
| Record | librecord / ffmpeg | ffmpeg `-c copy`, 10 slots |
| Sign | remote API + catvm | local `fp.data`, cached, warm process |
| Collect | compass cookies | same, explicit session jars |
| ASR | bundled | whisper.cpp / whisper CLI |
