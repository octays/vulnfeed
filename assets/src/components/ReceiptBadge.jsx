// 漏洞/公告列表行的回执结果标记：三维度齐备 > 部分确认 > 待回执 > 已关闭
export default function ReceiptBadge({ receipt }) {
  if (!receipt) return null
  let label, cls
  if (receipt.status !== 'open') { label = '已关闭'; cls = 'bg-green-100 text-green-800' }
  else if (receipt.dims_total > 0 && receipt.dims_done >= receipt.dims_total) { label = '三维度齐备'; cls = 'bg-green-100 text-green-800' }
  else if (receipt.dims_done > 0) { label = `确认中 ${receipt.dims_done}/${receipt.dims_total}`; cls = 'bg-amber-100 text-amber-800' }
  else { label = '待回执'; cls = 'bg-amber-100 text-amber-800' }
  return (
    <span
      className={`ml-2 inline-flex items-center px-2.5 py-0.5 rounded-full text-xs font-medium ${cls}`}
      title={`确认单 #${receipt.case_id} · ${receipt.responses} 条回执`}
    >
      {label}
    </span>
  )
}
