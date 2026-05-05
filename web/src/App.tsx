import { useEffect, useRef, useState } from 'react'
import {
  BarChart, Bar, LineChart, Line, PieChart, Pie, Cell, XAxis, YAxis, Tooltip,
  ResponsiveContainer, Legend, CartesianGrid,
} from 'recharts'

type Stats = {
  total_lines: number
  parsed_lines: number
  requests: number
  bytes: number
  by_ip: Record<string, number>
  by_status: Record<string, number>
  by_path: Record<string, number>
  by_hour: Record<string, number>
}

type ApiResponse = { stats: Stats; source: string }

const STATUS_HUE: Record<string, string> = {
  '2': '#ffffff',
  '3': '#98a8c8',
  '4': '#5878a0',
  '5': '#d28878',
}

function statusColor(code: string) {
  return STATUS_HUE[code[0]] || '#6b6b6b'
}

function topN(map: Record<string, number>, n: number): Array<[string, number]> {
  return Object.entries(map).sort((a, b) => b[1] - a[1]).slice(0, n)
}

function formatBytesPair(n: number): { value: string; unit?: string } {
  if (n < 1024) return { value: n.toLocaleString(), unit: 'B' }
  if (n < 1024 * 1024) return { value: (n / 1024).toFixed(1), unit: 'KB' }
  if (n < 1024 ** 3) return { value: (n / 1024 / 1024).toFixed(2), unit: 'MB' }
  return { value: (n / 1024 ** 3).toFixed(2), unit: 'GB' }
}

/* ------------------------------------------------------------------ */
/* Watermark                                                           */
/* ------------------------------------------------------------------ */
function Watermark() {
  const text = '  LOG / ANALYZER  ©  RICK  2026  ·  SPREZZATURAA  ·  ALL RIGHTS RESERVED  '
  const lines = Array.from({ length: 90 })
  return (
    <div className="watermark" aria-hidden="true">
      {lines.map((_, i) => (
        <div className="watermark-line" key={i}>
          {text.repeat(30)}
        </div>
      ))}
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Custom cursor — dot + lagging ring with mix-blend-mode              */
/* ------------------------------------------------------------------ */
function CustomCursor() {
  const dotRef = useRef<HTMLDivElement>(null)
  const ringRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const dot = dotRef.current!
    const ring = ringRef.current!
    let mx = window.innerWidth / 2
    let my = window.innerHeight / 2
    let rx = mx
    let ry = my
    let raf = 0
    let visible = false

    function loop() {
      rx += (mx - rx) * 0.18
      ry += (my - ry) * 0.18
      ring.style.transform = `translate3d(${rx}px, ${ry}px, 0) translate(-50%, -50%)`
      dot.style.transform = `translate3d(${mx}px, ${my}px, 0) translate(-50%, -50%)`
      raf = requestAnimationFrame(loop)
    }

    function onMove(e: MouseEvent) {
      mx = e.clientX
      my = e.clientY
      if (!visible) {
        ring.style.opacity = '1'
        dot.style.opacity = '1'
        visible = true
      }
    }

    function onLeaveWindow() {
      ring.style.opacity = '0'
      dot.style.opacity = '0'
      visible = false
    }

    function onOver(e: MouseEvent) {
      const t = e.target as HTMLElement
      const interactive = t.closest(
        'button, a, label, input, [data-cursor], .clickable, .recharts-pie-sector, tbody tr',
      )
      ring.classList.toggle('hover', !!interactive)
    }

    function onDown() {
      ring.classList.add('press')
    }
    function onUp() {
      ring.classList.remove('press')
    }

    document.addEventListener('mousemove', onMove)
    document.addEventListener('mouseover', onOver)
    document.addEventListener('mousedown', onDown)
    document.addEventListener('mouseup', onUp)
    document.addEventListener('mouseleave', onLeaveWindow)
    raf = requestAnimationFrame(loop)

    return () => {
      document.removeEventListener('mousemove', onMove)
      document.removeEventListener('mouseover', onOver)
      document.removeEventListener('mousedown', onDown)
      document.removeEventListener('mouseup', onUp)
      document.removeEventListener('mouseleave', onLeaveWindow)
      cancelAnimationFrame(raf)
    }
  }, [])

  return (
    <>
      <div ref={ringRef} className="cursor-ring" aria-hidden="true" />
      <div ref={dotRef} className="cursor-dot" aria-hidden="true" />
    </>
  )
}

/* ------------------------------------------------------------------ */
/* Glass interactions: per-element spotlight + 3D tilt                 */
/* ------------------------------------------------------------------ */
function useGlassInteractions(deps: unknown[]) {
  useEffect(() => {
    const els = Array.from(
      document.querySelectorAll<HTMLElement>('.glass, .cards .card'),
    )
    const cleanup: Array<() => void> = []

    els.forEach((el) => {
      let raf = 0
      let lastE: MouseEvent | null = null
      const tilt = el.dataset.tilt !== undefined

      function apply() {
        if (!lastE) return
        const r = el.getBoundingClientRect()
        const x = lastE.clientX - r.left
        const y = lastE.clientY - r.top
        el.style.setProperty('--mx', `${x}px`)
        el.style.setProperty('--my', `${y}px`)
        if (tilt) {
          const cx = (x / r.width - 0.5) * 2
          const cy = (y / r.height - 0.5) * 2
          el.style.transform = `perspective(1400px) rotateX(${(-cy * 1.6).toFixed(2)}deg) rotateY(${(cx * 1.6).toFixed(2)}deg) translateZ(0)`
        }
      }

      function onMove(e: MouseEvent) {
        lastE = e
        if (!raf) {
          raf = requestAnimationFrame(() => {
            raf = 0
            apply()
          })
        }
      }

      function onLeave() {
        if (tilt) el.style.transform = ''
      }

      el.addEventListener('mousemove', onMove)
      el.addEventListener('mouseleave', onLeave)
      cleanup.push(() => {
        el.removeEventListener('mousemove', onMove)
        el.removeEventListener('mouseleave', onLeave)
        if (raf) cancelAnimationFrame(raf)
      })
    })

    return () => cleanup.forEach((fn) => fn())
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps)
}

/* ------------------------------------------------------------------ */
/* Number count-up on mount/value-change                               */
/* ------------------------------------------------------------------ */
function useCountUp(target: number, duration = 1100): number {
  const [val, setVal] = useState(0)
  const startRef = useRef<number | null>(null)
  const fromRef = useRef(0)

  useEffect(() => {
    fromRef.current = val
    startRef.current = null
    let raf = 0
    function step(t: number) {
      if (startRef.current === null) startRef.current = t
      const p = Math.min((t - startRef.current) / duration, 1)
      const eased = 1 - Math.pow(1 - p, 4)
      setVal(fromRef.current + (target - fromRef.current) * eased)
      if (p < 1) raf = requestAnimationFrame(step)
    }
    raf = requestAnimationFrame(step)
    return () => cancelAnimationFrame(raf)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [target])

  return val
}

function CountInt({ value }: { value: number }) {
  const v = useCountUp(value)
  return <>{Math.round(v).toLocaleString()}</>
}

function CountFloat({ value, digits = 2 }: { value: number; digits?: number }) {
  const v = useCountUp(value)
  return <>{v.toFixed(digits)}</>
}

/* ------------------------------------------------------------------ */
/* Detail content for the click-to-expand panels                       */
/* ------------------------------------------------------------------ */
type DetailContent = {
  rail: string
  title: string
  headline: string
  headlineUnit?: string
  extras: Array<{ label: string; value: string }>
  plain: string
}

function cardDetail(key: 'lines' | 'parsed' | 'requests' | 'bytes', s: Stats): DetailContent {
  const bytes = formatBytesPair(s.bytes)
  const avgBytes = s.requests > 0 ? s.bytes / s.requests : 0
  const parseRate = (s.parsed_lines / Math.max(s.total_lines, 1)) * 100

  switch (key) {
    case 'lines':
      return {
        rail: 'Vital sign · 01',
        title: 'Lines read',
        headline: s.total_lines.toLocaleString(),
        extras: [
          { label: 'Parsed successfully', value: s.parsed_lines.toLocaleString() },
          { label: 'Skipped', value: (s.total_lines - s.parsed_lines).toLocaleString() },
          { label: 'Parse rate', value: `${parseRate.toFixed(2)}%` },
        ],
        plain:
          'The total number of lines the analyzer read from the file. Each line is one event — one moment when somebody asked the server for something. Lines that didn’t look like proper log entries (blank lines, malformed records) get quietly skipped.',
      }
    case 'parsed':
      return {
        rail: 'Vital sign · 02',
        title: 'Parsed lines',
        headline: s.parsed_lines.toLocaleString(),
        extras: [
          { label: 'Out of', value: s.total_lines.toLocaleString() },
          { label: 'Parse rate', value: `${parseRate.toFixed(2)}%` },
          { label: 'Skipped', value: (s.total_lines - s.parsed_lines).toLocaleString() },
        ],
        plain:
          'Lines that matched the NCSA Common Log Format pattern — IP, timestamp, request, status, size. The closer this is to the total lines, the cleaner your log file. A perfect 100% means every line was understood.',
      }
    case 'requests':
      return {
        rail: 'Vital sign · 03',
        title: 'Total requests',
        headline: s.requests.toLocaleString(),
        extras: [
          { label: 'Unique visitors (IPs)', value: Object.keys(s.by_ip).length.toLocaleString() },
          { label: 'Distinct pages hit', value: Object.keys(s.by_path).length.toLocaleString() },
          { label: 'Hours of activity', value: Object.keys(s.by_hour).length.toLocaleString() },
        ],
        plain:
          'Every parsed line is one request — one click, one API call, one image fetch. Some visitors generate dozens; some generate one. The status code chart and the IP / path tables on this page all draw from this same pool.',
      }
    case 'bytes':
      return {
        rail: 'Vital sign · 04',
        title: 'Bytes served',
        headline: bytes.value,
        headlineUnit: bytes.unit,
        extras: [
          { label: 'Total bytes', value: s.bytes.toLocaleString() },
          { label: 'Average per request', value: `${avgBytes.toFixed(0)} B` },
          { label: 'In megabytes', value: `${(s.bytes / 1024 / 1024).toFixed(2)} MB` },
        ],
        plain:
          'How much data the server pushed across the wire to satisfy every request. Larger numbers usually mean images or video are involved; small numbers suggest mostly text and JSON. The average gives you a feel for what a typical response looks like.',
      }
  }
}

const STATUS_CLASS: Record<string, { name: string; plain: string }> = {
  '2': {
    name: '2xx · Success',
    plain:
      'The request worked. The page loaded, the API returned data, the image came through. Most of a healthy site’s traffic should sit here.',
  },
  '3': {
    name: '3xx · Redirection',
    plain:
      'The browser got pointed to a different URL. Common reasons: a page moved, the site forces HTTPS, or the request matched a redirect rule. Generally fine and intentional.',
  },
  '4': {
    name: '4xx · Client error',
    plain:
      'The visitor asked for something the server couldn’t give them — a missing page (404), one that requires login (401), or one they aren’t allowed to see (403). Some are normal; a flood of them often means a scanner or a broken link.',
  },
  '5': {
    name: '5xx · Server error',
    plain:
      'The server tried to handle the request and broke. Each one is a real problem worth investigating — code crash, database timeout, downstream service down. Healthy sites have very few of these.',
  },
}

function formatHour(h: string): string {
  const n = parseInt(h, 10)
  if (n === 0) return 'Midnight'
  if (n === 12) return 'Noon'
  if (n < 12) return `${n} AM`
  return `${n - 12} PM`
}

function hourDetail(hour: string, s: Stats): DetailContent {
  const key = String(parseInt(hour, 10))
  const count = s.by_hour[key] ?? 0
  const pct = (count / Math.max(s.requests, 1)) * 100
  const hours = 24
  const avg = s.requests / hours
  const ratio = count / Math.max(avg, 1)

  let plain: string
  if (ratio > 1.5)
    plain = 'A traffic spike. Considerably busier than a typical hour — worth checking what was happening (a marketing send, a viral mention, or a scanner hammering the site).'
  else if (ratio > 1.1)
    plain = 'Above the daily average. The site was getting more attention than usual during this hour.'
  else if (ratio > 0.9)
    plain = 'A normal, average hour. Nothing unusual — the site behaved like itself.'
  else if (ratio > 0.5)
    plain = 'A quieter window. Fewer visitors than the average hour. Common during nights or off-peak times.'
  else
    plain = 'Very quiet. Almost no activity. Either the site was sleeping or you’re looking at a small site’s deep-night hour.'

  return {
    rail: `Hour · ${hour}:00`,
    title: `${formatHour(hour)} · ${hour}:00`,
    headline: count.toLocaleString(),
    headlineUnit: 'requests',
    extras: [
      { label: 'Share of day', value: `${pct.toFixed(2)}%` },
      { label: 'Hourly average', value: avg.toFixed(0) },
      { label: 'vs average', value: `${ratio >= 1 ? '+' : ''}${((ratio - 1) * 100).toFixed(0)}%` },
    ],
    plain,
  }
}

function pathDetail(path: string, s: Stats): DetailContent {
  const count = s.by_path[path] ?? 0
  const pct = (count / Math.max(s.requests, 1)) * 100
  const looksAdmin = /admin|wp-|\.env|\.git|phpmyadmin|server-status|\.aws/i.test(path)
  const looksApi = /^\/api\//.test(path)
  const looksStatic = /\.(css|js|png|jpg|jpeg|svg|ico|gif|woff)/i.test(path)

  let plain: string
  if (looksAdmin)
    plain = 'A suspicious path. Real visitors don’t normally go here — this is the kind of URL automated scanners probe looking for unprotected admin pages or leaked secrets. If counts are high, consider IP-blocking the visitors who hit it.'
  else if (looksApi)
    plain = 'An API endpoint — code talking to code. High counts here usually mean the frontend (or a third-party integration) is calling this route a lot.'
  else if (looksStatic)
    plain = 'A static asset — a stylesheet, script, image, or font. These get hit alongside page loads. Heavy ratios of static-to-page hits are normal for media-rich sites.'
  else
    plain = 'A regular page on the site. The number tells you how often it was viewed during the captured time window.'

  return {
    rail: `Endpoint`,
    title: path.length > 40 ? path.slice(0, 38) + '…' : path,
    headline: count.toLocaleString(),
    headlineUnit: 'requests',
    extras: [
      { label: 'Share of total', value: `${pct.toFixed(2)}%` },
      { label: 'Path length', value: `${path.length} chars` },
    ],
    plain,
  }
}

function statusDetail(code: string, s: Stats): DetailContent {
  const cls = STATUS_CLASS[code[0]] ?? { name: `${code}`, plain: 'Uncategorized status code.' }
  const count = s.by_status[code] ?? 0
  const pct = (count / Math.max(s.requests, 1)) * 100
  return {
    rail: `Status code · ${code}`,
    title: cls.name,
    headline: count.toLocaleString(),
    headlineUnit: 'requests',
    extras: [
      { label: 'Share of total', value: `${pct.toFixed(2)}%` },
      { label: 'Code class', value: `${code[0]}xx` },
      { label: 'Out of', value: s.requests.toLocaleString() },
    ],
    plain: cls.plain,
  }
}

function DetailPanel({
  detail,
  onClose,
}: {
  detail: DetailContent
  onClose: () => void
}) {
  return (
    <div className="detail-panel glass fade-up">
      <button className="detail-close" onClick={onClose} aria-label="Close">×</button>
      <div className="detail-head">
        <span className="rail-label">{detail.rail}</span>
        <h3 className="detail-title">{detail.title}</h3>
      </div>
      <div className="detail-headline">
        {detail.headline}
        {detail.headlineUnit && <span className="detail-unit">{detail.headlineUnit}</span>}
      </div>
      <div className="detail-grid">
        {detail.extras.map((e, i) => (
          <div className="detail-stat" key={i}>
            <span>{e.label}</span>
            <strong>{e.value}</strong>
          </div>
        ))}
      </div>
      <p className="plain detail-plain">{detail.plain}</p>
    </div>
  )
}

/* ------------------------------------------------------------------ */
/* Toast for click-to-copy                                              */
/* ------------------------------------------------------------------ */
function Toast({ msg }: { msg: string | null }) {
  return <div className={`toast ${msg ? 'show' : ''}`}>{msg}</div>
}

/* ------------------------------------------------------------------ */
/* Marquee ticker                                                       */
/* ------------------------------------------------------------------ */
function Ticker({ children }: { children: React.ReactNode }) {
  return (
    <div className="ticker">
      <div className="ticker-track">
        <div className="ticker-content">{children}</div>
        <div className="ticker-content" aria-hidden="true">{children}</div>
        <div className="ticker-content" aria-hidden="true">{children}</div>
      </div>
    </div>
  )
}

/* ================================================================== */
/* App                                                                 */
/* ================================================================== */
type SampleOption = {
  key: string
  label: string
  desc: string
}

const SAMPLE_OPTIONS: SampleOption[] = [
  { key: 'default', label: 'Standard',     desc: '500 lines · balanced demo' },
  { key: 'heavy',   label: 'Heavy traffic', desc: '5,000 lines · varied paths and IPs' },
  { key: 'attack',  label: 'Attack pattern', desc: '1,500 lines · suspicious bot activity' },
  { key: 'sparse',  label: 'Sparse',         desc: '80 lines · low-traffic small site' },
]

const SAMPLE_INFO: Record<string, {
  title: string;
  lede: string;
  body: string;
  detail?: string;
  plain: string;
}> = {
  default: {
    title: 'Balanced demo',
    lede: '500 lines, 24-hour spread.',
    body: 'A synthetic small-website log: ten IPs, twenty endpoints, traffic distributed across all hours of the day. Roughly two-thirds of requests succeed; the rest scatter across redirects, client errors, and server faults — designed to exercise every dimension of the analyzer at once.',
    detail: 'Use this when you want to see all four status-code classes simultaneously.',
    plain: 'Imagine a small website over the course of one day. Visitors clicked around — products, the shopping cart, the page behind the buttons. Most things worked. A few pages did not exist. A handful of unlucky moments threw errors. This is what an ordinary, healthy little website looks like.',
  },
  heavy: {
    title: 'Heavy traffic',
    lede: '5,000 lines, busier service.',
    body: 'Fifteen IPs hammering twenty-eight endpoints — the shape of a moderately busy small SaaS app. The hourly chart shows the natural daily spread; the top-paths table reveals which routes carry load (cart, checkout, API endpoints).',
    detail: 'Watch how the parallel rayon aggregation handles 10× the volume in roughly the same wall-clock time.',
    plain: 'Now picture that same website going viral, or an online store on a busy day. Ten times the activity. The hourly chart shows the rush hours; the popular-pages list shows what people came for — usually checkout, the cart, the things that actually drive a business.',
  },
  attack: {
    title: 'Attack pattern',
    lede: '1,500 lines, mostly hostile.',
    body: 'Characteristic bot reconnaissance — concentrated probes against /admin, /.env, /wp-login.php, /phpmyadmin from a tiny set of IPs. The status-code distribution skews heavily toward 401, 403, and 404 — the unmistakable signature of automated scanning.',
    detail: 'A working analyst would flag the top IPs here for rate-limiting or blocklisting.',
    plain: 'This is what it looks like when bad actors are scanning a website for ways to break in. Notice how a small handful of addresses keep trying the same suspicious pages — admin logins, hidden config files, things a normal visitor would never touch. Most attempts get rejected. A real operator seeing this would block those addresses immediately.',
  },
  sparse: {
    title: 'Sparse profile',
    lede: '80 lines, all 200 OK.',
    body: 'Eighty requests across three IPs and five paths, every one of them succeeding. The shape of a low-traffic personal blog or proof-of-concept site — useful for sanity-checking the analyzer against minimal input.',
    plain: 'A quiet personal site or hobby project. Three people stopped by, every page they asked for loaded fine, nothing broke. The kind of boring you actually want — a small site that simply works.',
  },
  uploaded: {
    title: 'Your own file',
    lede: 'User-supplied data.',
    body: 'You uploaded this. The same parallel parsing pipeline that powers the bundled samples ran against your file — regex per line, rayon fold-and-reduce across cores, JSON serialized back through the axum service.',
    plain: 'This is your own website’s log, run through the same analysis as the bundled samples. The numbers tell you who visited, what they looked at, when they came, and whether anything broke.',
  },
  none: {
    title: 'No data yet',
    lede: 'Awaiting input.',
    body: 'Pick one of the four bundled samples from the menu, or drop a .log file onto the upload bar. The analyzer accepts any file in NCSA Common Log Format — Apache, nginx, classic web-server logs.',
    detail: 'Larger logs are fine. The Rust core handles them quickly.',
    plain: 'Nothing loaded yet. A log file is just a record a website keeps every time someone visits — who they were, what page they wanted, whether it worked. Pick a sample from the menu to see what one looks like, or drop in your own.',
  },
}

export default function App() {
  const [stats, setStats] = useState<Stats | null>(null)
  const [source, setSource] = useState<string | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [selectedStatus, setSelectedStatus] = useState<string | null>(null)
  const [toast, setToast] = useState<string | null>(null)
  const [menuOpen, setMenuOpen] = useState(false)
  const [sampleKey, setSampleKey] = useState<string>('none')
  const [expandedCard, setExpandedCard] = useState<'lines' | 'parsed' | 'requests' | 'bytes' | null>(null)
  const [selectedHour, setSelectedHour] = useState<string | null>(null)
  const [selectedPath, setSelectedPath] = useState<string | null>(null)
  const [aiSummary, setAiSummary] = useState<{
    technical: string
    plain: string
    input_tokens: number
    output_tokens: number
    provider: string
    model: string
  } | null>(null)
  const [aiLoading, setAiLoading] = useState(false)
  const [aiError, setAiError] = useState<string | null>(null)

  useGlassInteractions([stats])

  // Per-panel shine phase offset so they don't all animate in lockstep.
  useEffect(() => {
    document.querySelectorAll<HTMLElement>('.glass').forEach((el, idx) => {
      el.style.setProperty('--shine-phase', `${-idx * 1.9}s`)
    })
  }, [stats, sampleKey, menuOpen])

  useEffect(() => {
    if (!menuOpen) return
    function onClick(e: MouseEvent) {
      const t = e.target as HTMLElement
      if (!t.closest('.menu-wrap')) setMenuOpen(false)
    }
    function onKey(e: KeyboardEvent) {
      if (e.key === 'Escape') setMenuOpen(false)
    }
    document.addEventListener('click', onClick)
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('click', onClick)
      document.removeEventListener('keydown', onKey)
    }
  }, [menuOpen])

  // Auto-generate the AI summary whenever a fresh log lands.
  useEffect(() => {
    if (stats && !aiSummary && !aiLoading && !aiError) {
      fetchAiSummary()
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stats])

  function flashToast(msg: string) {
    setToast(msg)
    setTimeout(() => setToast(null), 1400)
  }

  function copyToClipboard(text: string) {
    navigator.clipboard
      .writeText(text)
      .then(() => flashToast(`Copied · ${text}`))
      .catch(() => flashToast('Copy failed'))
  }

  function resetForNewData() {
    setSelectedStatus(null)
    setSelectedHour(null)
    setSelectedPath(null)
    setExpandedCard(null)
    setAiSummary(null)
    setAiError(null)
  }

  async function loadSample(key: string = 'default') {
    setLoading(true); setError(null); resetForNewData(); setMenuOpen(false)
    try {
      const url = key === 'default' ? '/api/sample' : `/api/sample?name=${encodeURIComponent(key)}`
      const res = await fetch(url)
      if (!res.ok) throw new Error(`HTTP ${res.status}`)
      const data: ApiResponse = await res.json()
      setStats(data.stats); setSource(data.source); setSampleKey(key)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  async function uploadFile(file: File) {
    setLoading(true); setError(null); resetForNewData()
    try {
      const fd = new FormData()
      fd.append('log', file)
      const res = await fetch('/api/analyze', { method: 'POST', body: fd })
      if (!res.ok) throw new Error(`HTTP ${res.status}: ${await res.text()}`)
      const data: ApiResponse = await res.json()
      setStats(data.stats); setSource(data.source); setSampleKey('uploaded')
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setLoading(false)
    }
  }

  async function fetchAiSummary() {
    if (!stats) return
    setAiLoading(true); setAiError(null)
    try {
      const res = await fetch('/api/ai-summary', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(stats),
      })
      if (!res.ok) {
        const text = await res.text()
        throw new Error(text || `HTTP ${res.status}`)
      }
      const data = await res.json()
      setAiSummary(data)
    } catch (e) {
      setAiError(e instanceof Error ? e.message : String(e))
    } finally {
      setAiLoading(false)
    }
  }

  function onDrop(e: React.DragEvent) {
    e.preventDefault(); setDragOver(false)
    const file = e.dataTransfer.files?.[0]
    if (file) uploadFile(file)
  }

  const statusData = stats
    ? Object.entries(stats.by_status)
        .sort((a, b) => Number(a[0]) - Number(b[0]))
        .map(([code, count]) => ({
          name: code,
          value: count,
          fill: statusColor(code),
          opacity: selectedStatus === null || selectedStatus === code ? 1 : 0.16,
        }))
    : []

  const hourData = stats
    ? Array.from({ length: 24 }, (_, h) => ({
        hour: String(h).padStart(2, '0'),
        requests: stats.by_hour[String(h)] || 0,
      }))
    : []

  const topIps = stats ? topN(stats.by_ip, 10) : []
  const topPaths = stats ? topN(stats.by_path, 10) : []

  const bytes = stats ? formatBytesPair(stats.bytes) : { value: '0', unit: 'B' }
  const parseRate = stats
    ? (stats.parsed_lines / Math.max(stats.total_lines, 1)) * 100
    : 0

  const tickerItems = stats ? (
    <>
      <span>● Live</span><i>·</i>
      <span><b>{stats.requests.toLocaleString()}</b> requests</span><i>·</i>
      <span>parse rate <b>{parseRate.toFixed(1)}%</b></span><i>·</i>
      <span>{Object.keys(stats.by_status).length} status classes</span><i>·</i>
      <span>{Object.keys(stats.by_ip).length} unique IPs</span><i>·</i>
      <span>{Object.keys(stats.by_path).length} endpoints</span><i>·</i>
      <span><b>{bytes.value}</b> {bytes.unit} served</span><i>·</i>
      <span>Vol. 01 · Q2 2026</span><i>·</i>
    </>
  ) : (
    <>
      <span>● Standing by</span><i>·</i>
      <span>Awaiting log file</span><i>·</i>
      <span>Editorial release · Vol. 01</span><i>·</i>
      <span>Built on Rust + Axum + React</span><i>·</i>
      <span>By Rick · Sprezzaturaa</span><i>·</i>
    </>
  )

  return (
    <div className="app">
      <CustomCursor />
      <Watermark />
      <Toast msg={toast} />

      <main className="container">
        <header className="masthead">
          <div>
            <p className="brand">Vol. 01 — Q2 / 2026</p>
            <h1>Log<span className="slash">/</span>Analyzer</h1>
            <p className="subtitle">
              An editorial study of web traffic intelligence —<br />
              parsed in parallel through Rust, served quietly through React.
            </p>
          </div>
          <div className="masthead-meta">
            <span className="pulse"><span className="dot" /> Live</span>
            <span>By Rick</span>
            <span>Sprezzaturaa</span>
          </div>
        </header>

        <Ticker>{tickerItems}</Ticker>

        <section
          className={`upload glass fade-up ${dragOver ? 'drag-over' : ''}`}
          onDragOver={(e) => { e.preventDefault(); setDragOver(true) }}
          onDragEnter={(e) => { e.preventDefault(); setDragOver(true) }}
          onDragLeave={() => setDragOver(false)}
          onDrop={onDrop}
        >
          <label className="btn">
            Choose Log File
            <input
              type="file"
              accept=".log,.txt,text/plain"
              onChange={(e) => e.target.files?.[0] && uploadFile(e.target.files[0])}
            />
          </label>
          <div className="menu-wrap">
            <button
              className="btn btn-primary"
              onClick={(e) => { e.stopPropagation(); setMenuOpen((o) => !o) }}
              disabled={loading}
              aria-expanded={menuOpen}
            >
              Use Sample
              <span className="caret">{menuOpen ? '▲' : '▼'}</span>
            </button>
            <div className={`menu ${menuOpen ? 'open' : ''}`} role="menu">
              {SAMPLE_OPTIONS.map((s) => (
                <button
                  key={s.key}
                  className="menu-item"
                  role="menuitem"
                  onClick={() => loadSample(s.key)}
                >
                  <span className="menu-item-label">{s.label}</span>
                  <span className="menu-item-desc">{s.desc}</span>
                </button>
              ))}
            </div>
          </div>
          <span className="drop-hint">or drop a .log file anywhere on this bar</span>
          <div
            className={`upload-status ${
              loading ? 'work' : error ? 'bad' : 'idle'
            }`}
          >
            {loading
              ? 'Analyzing…'
              : error
              ? `Error · ${error}`
              : stats
              ? 'Profile ready'
              : 'Awaiting input'}
          </div>
        </section>

        <div className="layout-2col">
          <div className="main-content">
            {!stats && !loading && !error && (
              <p className="empty fade-in">A blank canvas awaits your data.</p>
            )}

            {stats && (
              <>
            <p className="source-line fade-up">
              <span className="label">Source —</span>
              <strong>{source}</strong>
            </p>

            <div className="section-label fade-up">
              <span className="section-num">01</span>
              <span className="section-name">Vital signs</span>
              <div className="section-rule" />
            </div>

            {expandedCard ? (
              <DetailPanel
                detail={cardDetail(expandedCard, stats)}
                onClose={() => setExpandedCard(null)}
              />
            ) : (
              <div className="cards glass stagger fade-up">
                <Card label="Lines read" value={<CountInt value={stats.total_lines} />} onClick={() => setExpandedCard('lines')} />
                <Card
                  label="Parsed"
                  value={<CountInt value={stats.parsed_lines} />}
                  unit={
                    <>
                      <CountFloat value={parseRate} digits={1} />%
                    </>
                  }
                  onClick={() => setExpandedCard('parsed')}
                />
                <Card label="Requests" value={<CountInt value={stats.requests} />} onClick={() => setExpandedCard('requests')} />
                <Card
                  label="Bytes served"
                  value={
                    bytes.unit === 'B' ? (
                      <CountInt value={stats.bytes} />
                    ) : (
                      <CountFloat value={Number(bytes.value)} digits={bytes.unit === 'KB' ? 1 : 2} />
                    )
                  }
                  unit={bytes.unit}
                  onClick={() => setExpandedCard('bytes')}
                />
              </div>
            )}

            <div className="section-label fade-up">
              <span className="section-num">02</span>
              <span className="section-name">Distribution</span>
              <div className="section-rule" />
              {selectedStatus && (
                <button
                  className="filter-chip"
                  onClick={() => setSelectedStatus(null)}
                  data-cursor=""
                >
                  Filter · {selectedStatus} ✕
                </button>
              )}
            </div>

            <div className="split">
              <div className="panel glass fade-up" data-tilt>
                <div className="panel-head">
                  <h2 className="panel-title">Status codes</h2>
                  <span className="panel-tag">
                    {selectedStatus ? `Selected · ${selectedStatus}` : `Click to focus`}
                  </span>
                </div>
                <div className="chart-area">
                  <ResponsiveContainer width="100%" height={300}>
                    <PieChart>
                      <Pie
                        data={statusData}
                        dataKey="value"
                        nameKey="name"
                        innerRadius={64}
                        outerRadius={108}
                        paddingAngle={1.5}
                        stroke="rgba(10,9,8,0.6)"
                        strokeWidth={2}
                        onClick={(d) => {
                          const code = (d as { name: string }).name
                          setSelectedStatus(code === selectedStatus ? null : code)
                        }}
                      >
                        {statusData.map((s, i) => (
                          <Cell
                            key={i}
                            fill={s.fill}
                            fillOpacity={s.opacity}
                            style={{ cursor: 'pointer', transition: 'fill-opacity 0.4s' }}
                          />
                        ))}
                      </Pie>
                      <Tooltip />
                      <Legend
                        verticalAlign="bottom"
                        iconType="square"
                        iconSize={9}
                      />
                    </PieChart>
                  </ResponsiveContainer>
                </div>
                {selectedStatus && (
                  <DetailPanel
                    detail={statusDetail(selectedStatus, stats)}
                    onClose={() => setSelectedStatus(null)}
                  />
                )}
              </div>

              <div className="panel glass fade-up" data-tilt>
                <div className="panel-head">
                  <h2 className="panel-title">Hourly volume</h2>
                  <span className="panel-tag">{selectedHour ? `Selected · ${selectedHour}:00` : 'Click a point'}</span>
                </div>
                <div className="chart-area">
                  <ResponsiveContainer width="100%" height={300}>
                    <LineChart
                      data={hourData}
                      margin={{ top: 12, right: 16, left: -16, bottom: 0 }}
                      onClick={(state) => {
                        const label = (state as { activeLabel?: string } | null)?.activeLabel
                        if (label) setSelectedHour(label === selectedHour ? null : label)
                      }}
                      style={{ cursor: 'pointer' }}
                    >
                      <CartesianGrid strokeDasharray="2 4" vertical={false} />
                      <XAxis dataKey="hour" tickLine={false} interval={2} />
                      <YAxis tickLine={false} axisLine={false} />
                      <Tooltip cursor={{ stroke: 'rgba(255,255,255,0.25)', strokeWidth: 1 }} />
                      <Line
                        type="monotone"
                        dataKey="requests"
                        stroke="#ffffff"
                        strokeWidth={2}
                        dot={{ r: 3.5, fill: '#ffffff', strokeWidth: 0 }}
                        activeDot={{ r: 7, fill: '#ffffff', stroke: '#ffffff', strokeWidth: 2 }}
                      />
                    </LineChart>
                  </ResponsiveContainer>
                </div>
                {selectedHour && (
                  <DetailPanel
                    detail={hourDetail(selectedHour, stats)}
                    onClose={() => setSelectedHour(null)}
                  />
                )}
              </div>
            </div>

            <div className="section-label fade-up">
              <span className="section-num">03</span>
              <span className="section-name">Endpoint volume</span>
              <div className="section-rule" />
              <span className="hint-tag">Click a bar for details</span>
            </div>

            <div className="panel glass fade-up" data-tilt style={{ marginBottom: '2rem' }}>
              <div className="panel-head">
                <h2 className="panel-title">Top paths by volume</h2>
                <span className="panel-tag">{selectedPath ? 'Selected · 1' : 'Top 10'}</span>
              </div>
              <div className="chart-area">
                <ResponsiveContainer width="100%" height={Math.max(280, topPaths.length * 32)}>
                  <BarChart
                    data={topPaths.map(([p, c]) => ({ path: p, count: c }))}
                    layout="vertical"
                    margin={{ top: 4, right: 24, left: 8, bottom: 0 }}
                  >
                    <CartesianGrid strokeDasharray="2 4" horizontal={false} />
                    <XAxis type="number" tickLine={false} axisLine={false} />
                    <YAxis
                      type="category"
                      dataKey="path"
                      tickLine={false}
                      axisLine={false}
                      width={170}
                    />
                    <Tooltip cursor={{ fill: 'rgba(255,255,255,0.06)' }} />
                    <Bar
                      dataKey="count"
                      radius={[0, 1, 1, 0]}
                      maxBarSize={16}
                      onClick={(d) => {
                        const p = (d as { path: string }).path
                        setSelectedPath(p === selectedPath ? null : p)
                      }}
                      style={{ cursor: 'pointer' }}
                    >
                      {topPaths.map(([p], i) => (
                        <Cell
                          key={i}
                          fill="#ffffff"
                          fillOpacity={selectedPath === null || selectedPath === p ? 1 : 0.18}
                          style={{ transition: 'fill-opacity 0.4s' }}
                        />
                      ))}
                    </Bar>
                  </BarChart>
                </ResponsiveContainer>
              </div>
              {selectedPath && (
                <DetailPanel
                  detail={pathDetail(selectedPath, stats)}
                  onClose={() => setSelectedPath(null)}
                />
              )}
            </div>

            <div className="section-label fade-up">
              <span className="section-num">04</span>
              <span className="section-name">Traffic sources</span>
              <div className="section-rule" />
              <span className="hint-tag">Click row to copy</span>
            </div>

            <div className="split">
              <div className="panel glass fade-up" data-tilt>
                <div className="panel-head">
                  <h2 className="panel-title">Top IPs</h2>
                  <span className="panel-tag">Top 10</span>
                </div>
                <RankedTable data={topIps} keyHeader="Address" onPick={copyToClipboard} />
              </div>
              <div className="panel glass fade-up" data-tilt>
                <div className="panel-head">
                  <h2 className="panel-title">Top paths</h2>
                  <span className="panel-tag">Top 10</span>
                </div>
                <RankedTable data={topPaths} keyHeader="Endpoint" onPick={copyToClipboard} />
              </div>
            </div>
          </>
        )}
          </div>

          <aside className="sidebar">
            <AboutSample
              sampleKey={sampleKey}
              stats={stats}
              aiSummary={aiSummary}
              aiLoading={aiLoading}
              aiError={aiError}
              onRetry={fetchAiSummary}
            />
          </aside>
        </div>

        <footer>
          <span>Log/Analyzer · A study in Rust</span>
          <span className="accent">© Rick · Sprezzaturaa · MMXXVI</span>
        </footer>
      </main>
    </div>
  )
}

function Card({
  label,
  value,
  unit,
  onClick,
}: {
  label: string
  value: React.ReactNode
  unit?: React.ReactNode
  onClick?: () => void
}) {
  return (
    <div
      className={`card ${onClick ? 'clickable' : ''}`}
      onClick={onClick}
      role={onClick ? 'button' : undefined}
      tabIndex={onClick ? 0 : undefined}
      onKeyDown={(e) => {
        if (onClick && (e.key === 'Enter' || e.key === ' ')) {
          e.preventDefault()
          onClick()
        }
      }}
    >
      <div className="card-label">{label}</div>
      <div className="card-value">
        {value}
        {unit && <span className="unit">{unit}</span>}
      </div>
      {onClick && <span className="card-expand-hint">↗</span>}
    </div>
  )
}

function AboutSample({
  sampleKey,
  stats,
  aiSummary,
  aiLoading,
  aiError,
  onRetry,
}: {
  sampleKey: string
  stats: Stats | null
  aiSummary: {
    technical: string
    plain: string
    input_tokens: number
    output_tokens: number
    provider: string
    model: string
  } | null
  aiLoading: boolean
  aiError: string | null
  onRetry: () => void
}) {
  const info = SAMPLE_INFO[sampleKey] ?? SAMPLE_INFO.none
  const hasData = !!stats

  const railLabel = hasData ? 'AI insights' : 'About this sample'
  const title = hasData ? info.title : 'No data yet'

  return (
    <div className="about-sample glass">
      <div className="about-sample-head">
        <span className="rail-label">{railLabel}</span>
      </div>
      <div className="about-sample-body">
        {!hasData && (
          <>
            <h3>{title}</h3>
            <p className="lede">{info.lede}</p>
            <p>{info.body}</p>
            {info.detail && <p className="detail">{info.detail}</p>}
            <div className="plain-section">
              <span className="plain-label">In plain words</span>
              <p className="plain">{info.plain}</p>
            </div>
          </>
        )}

        {hasData && aiLoading && (
          <div className="ai-inline-loading">
            <span className="ai-spinner" />
            <span>Reading the log…</span>
          </div>
        )}

        {hasData && aiError && (
          <div className="ai-inline-error">
            <h3>Couldn't reach the AI</h3>
            <p>{aiError}</p>
            <p className="detail">
              Need a free key? Sign up at{' '}
              <a href="https://console.groq.com" target="_blank" rel="noopener noreferrer">
                console.groq.com
              </a>
              , then set <code>GROQ_API_KEY</code> in the server environment and
              restart it.
            </p>
            <button className="filter-chip" onClick={onRetry}>Retry</button>
          </div>
        )}

        {hasData && aiSummary && !aiLoading && !aiError && (
          <>
            <h3>Technical reading</h3>
            <p>{aiSummary.technical}</p>
            <div className="plain-section">
              <span className="plain-label">In plain words</span>
              <p className="plain">{aiSummary.plain}</p>
            </div>
          </>
        )}

        {stats && (
          <ul className="quick-stats">
            <li>
              <span>Lines</span>
              <strong>{stats.total_lines.toLocaleString()}</strong>
            </li>
            <li>
              <span>Unique IPs</span>
              <strong>{Object.keys(stats.by_ip).length}</strong>
            </li>
            <li>
              <span>Endpoints</span>
              <strong>{Object.keys(stats.by_path).length}</strong>
            </li>
            <li>
              <span>Status classes</span>
              <strong>{Object.keys(stats.by_status).length}</strong>
            </li>
          </ul>
        )}
      </div>
    </div>
  )
}

function RankedTable({
  data,
  keyHeader,
  onPick,
}: {
  data: Array<[string, number]>
  keyHeader: string
  onPick: (k: string) => void
}) {
  return (
    <table>
      <thead>
        <tr>
          <th style={{ width: '2.5rem' }}>№</th>
          <th>{keyHeader}</th>
          <th className="num">Count</th>
        </tr>
      </thead>
      <tbody>
        {data.map(([k, v], i) => (
          <tr key={k} onClick={() => onPick(k)} className="clickable">
            <td className="rank">{String(i + 1).padStart(2, '0')}</td>
            <td className="mono">{k}</td>
            <td className="num">{v.toLocaleString()}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
