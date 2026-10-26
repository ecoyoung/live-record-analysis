# Iteration log — 直播录制分析

Compare against 爱复盘 desktop: keep probe / auto-record / 10-way ingest / ASR / compass+qianchuan / danmaku. Drop Chromium UI, SaaS login, cloud.

## Direction

1. **CPU** — no Cef; ffmpeg copy-codec, 1 thread, nice 10; default HD1; skip enter while recording; warm Node signer; batch SQLite danmaku; serialize whisper.
2. **Stability** — cap 10 recorders; abort tiny FLV; no reconnect-at-eof; re-probe live rooms every 4 ticks; 180s ASR kill.
3. **Product loop** — live → record + danmaku → collect dashboards → stop → `.asr.txt` + report JSON.

## Key points vs 爱复盘

| | 爱复盘 | This tree |
|---|---|---|
| UI | CefSharp Chrome | none |
| Record | librecord / ffmpeg | ffmpeg `-c copy`, 10 slots |
| Sign | remote API + catvm per connect | local `fp.data`, 5 min cache, warm process |
| Collect | compass cookies | same jars + `/api/config/auth` |
| ASR | bundled | whisper.cpp, one job at a time |

## Commits (this pass)

1. `ITERATION.md` — product vs 爱复盘
2. Cap 10 concurrent recorders, skip enter while ingesting
3. Low-CPU ffmpeg (threads 1, nice 10, abort tiny files)
4. Default HD1 with `LIVE_QUALITY` ladder
5. Batch danmaku SQLite writes
6. Warm Node `frontierSign` daemon
7. Serialize ASR, whisper 2-thread cap, `.asr.txt`
8. Compass / Qianchuan auth status API
9. Re-probe recording rooms every 4 ticks; slot health
10. README / this log for 10-way ops
11. CPU roles: control normal; ffmpeg nice 10; signer nice 5; ASR nice 15
