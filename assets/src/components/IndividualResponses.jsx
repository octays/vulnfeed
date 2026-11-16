import { useState } from 'react'
import api from '../lib/api'
import { localDateTime } from '../lib/format'
import { RECEIPT_STATUSES, TASK_STATUSES, ownersDisplay } from '../lib/receipt'
const input = 'px-3 py-2 border rounded w-full'
const legacyStatuses = { investigating: '待排查', unaffected: '无影响', affected: '有影响' }

export default function IndividualResponses({ responses, dimensions, open, run, refresh }) {
  const [draft, setDraft] = useState(null)
  if (!responses.length) return <p className="text-sm text-gray-500">暂无回执</p>
  return <div className="space-y-2">{responses.map(r => <div key={r.id} className="p-3 bg-gray-50 rounded text-sm">
    {draft?.id === r.id ? <div className="space-y-2">
      <div className="grid gap-2 md:grid-cols-2">
        <input className={input} aria-label="提交人" value={draft.display_name} onChange={e=>setDraft({...draft,display_name:e.target.value})}/>
        <select className={input} value={draft.dimension || ''} onChange={e=>setDraft({...draft,dimension:e.target.value||null})}>
          <option value="">维度不变{r.dimension_name ? `（${r.dimension_name}）` : ''}</option>
          {(dimensions||[]).map(d=><option key={d.slug} value={d.slug}>{d.name}</option>)}
        </select>
      </div>
      <select className={input} value={draft.status} onChange={e=>setDraft({...draft,status:e.target.value})}>
        <optgroup label="五选一">{Object.entries(RECEIPT_STATUSES).map(([v,label])=><option key={v} value={v}>{label}</option>)}</optgroup>
        <optgroup label="旧三态（历史数据）">{Object.entries(legacyStatuses).map(([v,label])=><option key={v} value={v}>{label}</option>)}</optgroup>
      </select>
      <textarea className={input} aria-label="备注" placeholder="备注（可空）" value={draft.details} onChange={e=>setDraft({...draft,details:e.target.value})}/>
      <div><button className="text-indigo-700" onClick={()=>run(async()=>{await api.put(`/receipts/responses/${r.id}`,draft);setDraft(null);await refresh()})}>保存</button> <button onClick={()=>setDraft(null)}>取消</button></div>
    </div> : <>
      <p><b>{r.dimension_name || '—（旧个人回执）'}</b> · {RECEIPT_STATUSES[r.status] || TASK_STATUSES[r.status] || r.status}{(() => { const owners = ownersDisplay(r.owners); return owners ? `（负责人：${owners}）` : '' })()} · {localDateTime(r.updated_at)}</p>
      <p className="text-gray-600">提交人：{r.display_name || r.operator_open_id}{r.team ? ` · ${r.team}` : ''}</p>
      {!r.display_name && <p className="text-amber-700">{r.name_lookup_failed ? '暂无法获取姓名，请检查飞书应用的通讯录权限；当前显示 Open ID。' : '正在自动获取姓名，暂显示 Open ID。'} {r.name_lookup_failed && <button className="text-indigo-700" onClick={()=>run(async()=>{await api.post(`/receipts/responses/${r.id}/retry-name`);await refresh()})}>重新获取</button>}</p>}
      {r.details && <p className="whitespace-pre-wrap">{r.details}</p>}
      {open && <div className="flex gap-3 mt-1"><button className="text-indigo-700" onClick={()=>setDraft({...r, dimension:r.dimension||null})}>修改</button><button className="text-red-700" onClick={()=>{if(window.confirm('删除这条回执？删除后该维度视为未回执。'))run(async()=>{await api.delete(`/receipts/responses/${r.id}`,{data:{version:r.version}});await refresh()})}}>删除</button></div>}
    </>}
  </div>)}</div>
}
