const DAY_MS = 24 * 60 * 60 * 1000;

export const WEEKDAY_SHORT = ["周日", "周一", "周二", "周三", "周四", "周五", "周六"];

/* --------------------------------------------------------------------------
   时区感知的时间原语
   --------------------------------------------------------------------------
   为什么要显式传偏移：归日、小时分布、连击都依赖"用户所在时区的哪一天"。
   直接用 `getHours()` / `setHours(0,0,0,0)` 取的是**运行环境**的时区，一旦用户
   设置了 profile.timezone 或跨时区使用，统计口径就会漂移。

   约定：offsetMinutes 为"东正西负"的分钟数（UTC+8 = 480）。
   -------------------------------------------------------------------------- */

/** 把时刻按给定偏移平移到"该时区的钟表时间"，之后用 getUTC* 读取即可 */
function shiftByOffset(date: Date | string, offsetMinutes: number): Date {
  return new Date(new Date(date).getTime() + offsetMinutes * 60_000);
}

function pad2(value: number): string {
  return `${value}`.padStart(2, "0");
}

/** 运行环境当前的时区偏移（分钟） */
export function localOffsetMinutes(at: Date = new Date()): number {
  return -at.getTimezoneOffset();
}

/**
 * 把 IANA 时区（如 "Asia/Shanghai"）换算成给定时刻的偏移分钟数。
 * 无法识别时回退到运行环境偏移，保证不会因为配置错误而崩溃。
 */
export function offsetMinutesForTimeZone(timeZone: string, at: Date = new Date()): number {
  try {
    const parts = new Intl.DateTimeFormat("en-US", {
      timeZone,
      hour12: false,
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    }).formatToParts(at);

    const pick = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? "0");
    const hour = pick("hour") % 24; // "24" 归一为 0
    const asUtc = Date.UTC(
      pick("year"),
      pick("month") - 1,
      pick("day"),
      hour,
      pick("minute"),
      pick("second"),
    );
    return Math.round((asUtc - at.getTime()) / 60_000);
  } catch {
    return localOffsetMinutes(at);
  }
}

/** 给定时区下的本地日期（YYYY-MM-DD） */
export function isoDateIn(date: Date | string = new Date(), offsetMinutes = localOffsetMinutes()): string {
  const d = shiftByOffset(date, offsetMinutes);
  return `${d.getUTCFullYear()}-${pad2(d.getUTCMonth() + 1)}-${pad2(d.getUTCDate())}`;
}

/** 给定时区下的小时（0–23） */
export function hourIn(date: Date | string, offsetMinutes = localOffsetMinutes()): number {
  return shiftByOffset(date, offsetMinutes).getUTCHours();
}

/** 给定时区下"当天零点"对应的绝对时刻 */
export function startOfDayIn(
  date: Date | string = new Date(),
  offsetMinutes = localOffsetMinutes(),
): Date {
  const d = shiftByOffset(date, offsetMinutes);
  const midnightUtc = Date.UTC(d.getUTCFullYear(), d.getUTCMonth(), d.getUTCDate());
  return new Date(midnightUtc - offsetMinutes * 60_000);
}

/** 给定时区下的加天数 */
export function addDaysIn(
  date: Date | string,
  days: number,
  offsetMinutes = localOffsetMinutes(),
): Date {
  return new Date(startOfDayIn(date, offsetMinutes).getTime() + days * DAY_MS);
}

/** 给定时区下两个时刻相差的自然日数 */
export function daysBetweenIn(
  a: Date | string,
  b: Date | string,
  offsetMinutes = localOffsetMinutes(),
): number {
  return Math.round(
    (startOfDayIn(b, offsetMinutes).getTime() - startOfDayIn(a, offsetMinutes).getTime()) / DAY_MS,
  );
}

/** 给定时区下最近 n 天的 ISO 日期，来源最早在前 */
export function lastNDaysIn(
  n: number,
  from: Date | string = new Date(),
  offsetMinutes = localOffsetMinutes(),
): string[] {
  const base = startOfDayIn(from, offsetMinutes);
  return Array.from({ length: n }, (_, i) =>
    isoDateIn(new Date(base.getTime() + (i - (n - 1)) * DAY_MS), offsetMinutes),
  );
}

export function startOfDay(date: Date | string = new Date()) {
  const d = new Date(date);
  d.setHours(0, 0, 0, 0);
  return d;
}

export function addDays(date: Date | string, days: number) {
  const d = new Date(date);
  d.setDate(d.getDate() + days);
  return d;
}

export function isoDate(date: Date | string = new Date()) {
  const d = new Date(date);
  const month = `${d.getMonth() + 1}`.padStart(2, "0");
  const day = `${d.getDate()}`.padStart(2, "0");
  return `${d.getFullYear()}-${month}-${day}`;
}

export function isoDateTime(date: Date = new Date()) {
  return date.toISOString();
}

export function daysBetween(a: Date | string, b: Date | string) {
  return Math.round((startOfDay(b).getTime() - startOfDay(a).getTime()) / DAY_MS);
}

/** 生成最近 n 天的 ISO 日期，来源最早在前 */
export function lastNDays(n: number, from: Date | string = new Date()) {
  const base = startOfDay(from);
  return Array.from({ length: n }, (_, i) => isoDate(addDays(base, i - (n - 1))));
}

export function formatDateShort(date: string | Date) {
  const d = new Date(date);
  return `${d.getMonth() + 1}月${d.getDate()}日`;
}

export function formatMonthDay(date: string | Date) {
  const d = new Date(date);
  return `${d.getMonth() + 1}/${d.getDate()}`;
}

export function formatWeekday(date: string | Date) {
  return WEEKDAY_SHORT[new Date(date).getDay()];
}

export function formatRelativeDay(date: string | Date) {
  const diff = daysBetween(new Date(), date);
  if (diff === 0) return "今天";
  if (diff === 1) return "明天";
  if (diff === -1) return "昨天";
  if (diff < -1) return `${Math.abs(diff)} 天前`;
  return `${diff} 天后`;
}

export function formatClock(iso: string) {
  const d = new Date(iso);
  return `${`${d.getHours()}`.padStart(2, "0")}:${`${d.getMinutes()}`.padStart(2, "0")}`;
}

export function formatRelativeTime(iso: string) {
  const diff = Date.now() - new Date(iso).getTime();
  const minutes = Math.round(diff / 60000);
  if (minutes < 1) return "刚刚";
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days} 天前`;
  return formatDateShort(iso);
}

export function isSameDay(a: string | Date, b: string | Date) {
  return isoDate(a) === isoDate(b);
}

export function greeting(date: Date = new Date()) {
  const hour = date.getHours();
  if (hour < 6) return "夜深了";
  if (hour < 11) return "早上好";
  if (hour < 14) return "中午好";
  if (hour < 18) return "下午好";
  return "晚上好";
}
