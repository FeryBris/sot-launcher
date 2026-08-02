const MONTH_NAMES = [
  'January',
  'February',
  'March',
  'April',
  'May',
  'June',
  'July',
  'August',
  'September',
  'October',
  'November',
  'December',
]

const BRATISLAVA_TZ = 'Europe/Bratislava'

/** Pulls the wall-clock fields (as numbers) that `date` corresponds to in
 *  Bratislava specifically - not this machine's own system timezone, which
 *  could be set to anything regardless of where the launcher's actually
 *  run from. hourCycle 'h23' avoids Intl's midnight-as-"24" quirk under
 *  'h24'. */
function bratislavaParts(date: Date) {
  const parts = new Intl.DateTimeFormat('en-US', {
    timeZone: BRATISLAVA_TZ,
    year: 'numeric',
    month: 'numeric',
    day: 'numeric',
    hour: 'numeric',
    minute: 'numeric',
    second: 'numeric',
    hourCycle: 'h23',
  }).formatToParts(date)

  const get = (type: string) => Number(parts.find((p) => p.type === type)?.value ?? 0)
  return {
    year: get('year'),
    month: get('month'),
    day: get('day'),
    hour: get('hour'),
    minute: get('minute'),
    second: get('second'),
  }
}

/** House date/time format for the whole launcher: "HH:MM:SS D. MonthName
 *  YYYY" - e.g. "14:36:59 28. February 2026" - always in Europe/Bratislava
 *  (this guild runs on Slovak time), regardless of the viewing machine's
 *  own system timezone. */
export function formatDateTime(date: Date): string {
  const { year, month, day, hour, minute, second } = bratislavaParts(date)
  const pad = (n: number) => String(n).padStart(2, '0')
  const time = `${pad(hour)}:${pad(minute)}:${pad(second)}`
  return `${time} ${day}. ${MONTH_NAMES[month - 1]} ${year}`
}
