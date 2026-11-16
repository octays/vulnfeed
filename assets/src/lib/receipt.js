// 维度回执（SOP v24）：五选一选项与状态标签的共享定义
export const RECEIPT_STATUSES = {
  none: '无影响，无需处置流程',
  ops: '已规划运维处理',
  iter: '已规划迭代处理',
  both: '已规划运维 + 迭代处理',
  special: '已规划专项处理（含客户侧）',
}

export const TASK_STATUSES = {
  pending: '待确认',
  in_progress: '部分确认',
  done: '三维度齐备',
  // 旧三态与处置流（历史数据展示用）
  investigating: '待排查',
  unaffected: '无影响',
  affected: '有影响',
  awaiting_verification: '待验证',
  verified: '已验证',
  open: '进行中',
  closed: '已关闭',
  deleted: '已删除',
}

// owners 列存储 [{"open_id":"ou_..","name":"张三"}]，姓名未解析时显示 open_id
export function ownersDisplay(ownersJson) {
  try {
    const list = JSON.parse(ownersJson || '[]')
    return (Array.isArray(list) ? list : []).map(o => o.name || o.open_id).filter(Boolean).join('、')
  } catch (_) { return '' }
}
