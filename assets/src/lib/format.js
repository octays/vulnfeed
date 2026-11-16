// 后端 CURRENT_TIMESTAMP 为 UTC（"YYYY-MM-DD HH:MM:SS"），展示时转为浏览器本地时区
export const localDateTime = value => {
  if (!value) return value
  const date = new Date(String(value).replace(' ', 'T') + 'Z')
  return isNaN(date.getTime()) ? value : date.toLocaleString('zh-CN', { hour12: false })
}
