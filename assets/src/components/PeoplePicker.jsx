import { useMemo, useRef, useState } from 'react'

// Search-and-pick one person from the tenant directory. Selecting fills both
// the display name and the open_id — no manual typing. Pasting a raw ou_…
// still works as a fallback for people outside the contact scope.
export default function PeoplePicker({ contacts, value, onChange, placeholder }) {
  const [query, setQuery] = useState('')
  const [open, setOpen] = useState(false)
  const blurTimer = useRef(null)
  const openId = (value?.open_id || '').trim()
  const selected = contacts.find(c => c.open_id === openId)

  const hits = useMemo(() => {
    const q = query.trim().toLowerCase()
    if (!q) return contacts.slice(0, 20)
    return contacts
      .filter(c =>
        (c.name || '').toLowerCase().includes(q)
        || (c.department || '').toLowerCase().includes(q)
        || c.open_id.toLowerCase().includes(q))
      .slice(0, 20)
  }, [contacts, query])

  const showText = open
    ? query
    : selected
      ? (selected.department ? `${selected.name} · ${selected.department}` : selected.name)
      : openId

  return (
    <div className="relative" onBlur={() => { blurTimer.current = setTimeout(() => setOpen(false), 150) }} onFocus={() => clearTimeout(blurTimer.current)}>
      <input
        className="px-3 py-2 border rounded w-full text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
        placeholder={placeholder || '搜索姓名 / 部门并选择'}
        value={showText}
        onChange={e => { setQuery(e.target.value); setOpen(true) }}
        onFocus={() => { setQuery(''); setOpen(true) }}
      />
      {(selected || openId) && (
        <button type="button" className="absolute right-2 top-1/2 -translate-y-1/2 text-gray-400 hover:text-gray-600"
          title="清除"
          onClick={() => { onChange({ open_id: '', name: '' }); setQuery(''); setOpen(false) }}>✕</button>
      )}
      {open && (
        <ul className="absolute z-20 left-0 right-0 mt-1 max-h-60 overflow-auto bg-white border rounded shadow-lg text-sm">
          {hits.map(c => (
            <li key={c.open_id}>
              <button type="button" className="w-full text-left px-3 py-2 hover:bg-indigo-50"
                onMouseDown={() => { onChange({ open_id: c.open_id, name: c.name }); setQuery(''); setOpen(false) }}>
                <span className="font-medium">{c.name || c.open_id}</span>
                {c.department && <span className="text-gray-500"> · {c.department}</span>}
              </button>
            </li>
          ))}
          {hits.length === 0 && (
            <li className="px-3 py-2 text-gray-500">
              {query.trim().startsWith('ou_')
                ? <button type="button" className="text-indigo-700 hover:underline"
                    onMouseDown={() => { onChange({ open_id: query.trim(), name: '' }); setQuery(''); setOpen(false) }}>
                    使用原始 Open ID：{query.trim()}
                  </button>
                : '没有匹配的成员，可尝试其他关键词，或直接粘贴 ou_ 开头的 Open ID'}
            </li>
          )}
        </ul>
      )}
      {selected && <p className="mt-1 text-xs text-gray-400">{selected.open_id}</p>}
    </div>
  )
}
