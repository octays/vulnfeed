import api from '../lib/api'
import { useEffect, useState } from 'react'

const Section = ({ n, title, tag, children }) => (
  <section className="p-4 bg-white rounded-lg border border-gray-200">
    <div className="flex items-center gap-2 mb-3">
      <span className="px-2 py-0.5 rounded text-xs font-semibold bg-indigo-100 text-indigo-700">{n}</span>
      <h2 className="text-base font-semibold text-gray-900">{title}</h2>
      {tag && <span className="px-2 py-0.5 rounded text-xs bg-amber-50 text-amber-700 border border-amber-200">{tag}</span>}
    </div>
    {children}
  </section>
)

const Chip = ({ tone = 'slate', children }) => {
  const tones = {
    slate: 'bg-slate-100 text-slate-700', blue: 'bg-blue-100 text-blue-700', green: 'bg-green-100 text-green-700',
    amber: 'bg-amber-100 text-amber-700', red: 'bg-red-100 text-red-700', violet: 'bg-violet-100 text-violet-700',
  }
  return <span className={`inline-flex items-center gap-1 px-2 py-0.5 rounded-full text-xs font-medium ${tones[tone]}`}>{children}</span>
}

const OptRow = ({ title, who, multi }) => (
  <div className="flex items-start gap-2 py-1.5 border-b border-gray-100 last:border-0">
    <span className="mt-0.5 w-1.5 h-1.5 rounded-full bg-indigo-400 shrink-0" />
    <div className="flex-1 min-w-0">
      <p className="text-sm font-medium text-gray-800">{title}</p>
      {who && <p className="text-xs text-gray-500">{who}{multi ? '（多选 1–5 人）' : '（单选 1 人）'}</p>}
    </div>
  </div>
)

export default function SopPage() {
  const [dims, setDims] = useState([])
  useEffect(() => { api.get('/receipts/dimensions').then(r => setDims(r.data.data || [])).catch(() => {}) }, [])

  return <div className="max-w-4xl mx-auto space-y-4">
    <div className="text-center py-4">
      <p className="text-xs text-indigo-600 font-medium tracking-wide">SOP v24 · 三维度回执规则</p>
      <h1 className="text-2xl font-bold text-gray-900 mt-1">安全漏洞处理流程速查</h1>
      <p className="text-sm text-gray-500 mt-1">哪个环节、谁、做什么、走哪条轨、多久</p>
    </div>

    <div className="p-4 bg-red-50 border border-red-200 rounded-lg">
      <h2 className="text-sm font-bold text-red-800 mb-2">⚠️ 处置红线（五条不可绕过）</h2>
      <ul className="space-y-1.5 text-sm text-red-900">
        <li>• 回执统一走确认卡片表单，文本回复只是讨论、不算回执</li>
        <li>• 回执固定 @ 三个维度负责人分发；<b>三个维度全部确认「无影响」或「已有处置流程」才能关闭确认单</b></li>
        <li>• <b>回执确认只需一次</b>：记录了处置流程 + 负责人即算该维度完成，后续闭环由处置流程负责人推动</li>
        <li>• 临时缓解规则必须持久化，尤其是没有迭代跟进的自营环境</li>
        <li>• 关闭 ≠ 已修复：是否完成处置要结合资产信息和人工验证判断</li>
      </ul>
    </div>

    <Section n="一" title="主流程五阶段">
      <div className="overflow-x-auto">
        <table className="w-full text-sm min-w-[720px]">
          <thead><tr className="text-left text-xs text-gray-500 border-b">
            <th className="py-2 pr-3 w-[10%]">阶段</th>
            <th className="py-2 pr-3 w-[22%]">责任人</th>
            <th className="py-2 pr-3 w-[30%]">做什么</th>
            <th className="py-2">关键规则</th>
          </tr></thead>
          <tbody className="divide-y divide-gray-100">
            {[
              ['1 采集推送', '平台自动（管理员维护配置）', '多源采集→去重→技术栈打标→按群订阅投递卡片', '自动确认需同步开关+新记录+披露日期当天及以后；历史手动发起'],
              ['2 影响确认', '@ 三维度负责人', '各维度排查影响，在卡片上五选一+选处置负责人', '已确认无影响或已有处置流程的不用再 @'],
              ['3 分轨决策', '安全人员+维度负责人', '无影响→三维度齐备后关闭；有影响→按第二节选轨', '拿不准先排查再回执，不能不回复'],
              ['4 处置执行', '运维轨 / 迭代轨 / 客户侧轨', '由处置流程负责人推动闭环', '回执确认只需一次；严重漏洞同时启动专项群'],
              ['5 关闭复核', '平台管理员', '三维度齐备后填写说明并关闭', '关闭后不能再提交/修改回执'],
            ].map(([n, w, d, r]) => (
              <tr key={n} className="align-top">
                <td className="py-2 pr-3 font-medium text-indigo-700">{n}</td>
                <td className="py-2 pr-3 text-gray-700">{w}</td>
                <td className="py-2 pr-3 text-gray-600">{d}</td>
                <td className="py-2 text-gray-600">{r}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>

    <Section n="二" title="分轨决策">
      <div className="overflow-x-auto">
        <table className="w-full text-sm min-w-[560px]">
          <thead><tr className="text-left text-xs text-gray-500 border-b">
            <th className="py-2 pr-3 w-[36%]">情况</th><th className="py-2 pr-3 w-[18%]">轨道</th><th className="py-2">责任方</th>
          </tr></thead>
          <tbody className="divide-y divide-gray-100">
            {[
              ['自营/线上环境暴露', '运维轨', '运维负责人'],
              ['产品代码/固件需修改', '迭代轨', '产线开发'],
              ['两者兼有', '双轨并行', '运维先缓解，产线迭代跟进'],
              ['客户部署环境（BYOI）', '客户侧轨', '产线+客户侧支持'],
              ['严重级/在野利用/客户问询', '加启专项群', '安全人员+项目协调'],
            ].map(([c, t, o]) => (
              <tr key={c}><td className="py-2 pr-3 font-medium text-gray-800">{c}</td><td className="py-2 pr-3"><Chip tone={t.includes('运维')?'blue':t.includes('迭代')?'green':t.includes('双轨')?'amber':'violet'}>{t}</Chip></td><td className="py-2 text-gray-600">{o}</td></tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>

    <Section n="三" title="回执五选项（处置流程+负责人）" tag="卡片表单">
      <p className="text-sm text-gray-600 mb-2">在确认卡片上五选一，处置负责人从飞书通讯录选取（不限于固定名单），平台记录所选负责人以便追溯：</p>
      <div className="rounded-lg border border-gray-200 divide-y divide-gray-100">
        <OptRow title="无影响，无需处置流程" who="— 无需指定负责人" />
        <OptRow title="已规划运维处理" who="从飞书通讯录选择负责人" />
        <OptRow title="已规划迭代处理" who="从飞书通讯录选择负责人" />
        <OptRow title="已规划运维+迭代处理" who="从飞书通讯录选择负责人" multi />
        <OptRow title="已规划专项处理（含客户侧）" who="从飞书通讯录选择负责人" multi />
      </div>
      <div className="mt-3 p-3 bg-green-50 border border-green-200 rounded-lg text-sm">
        <b className="text-green-800">回执确认只需一次</b><span className="text-green-700"> — 记录了处置流程+负责人即算该维度完成，后续闭环由处置流程负责人推动，不用回平台再回执确认。</span>
      </div>
    </Section>

    <Section n="四" title="当前维度配置" tag="实时">
      {dims.map(d => (
        <div key={d.id} className={`flex items-center justify-between py-2 border-b border-gray-100 last:border-0 ${d.enabled ? '' : 'opacity-50'}`}>
          <div className="flex items-center gap-2">
            <Chip tone={d.slug === 'p' ? 'blue' : d.slug === 'novo' ? 'violet' : 'green'}>{d.name}</Chip>
            <span className="text-sm text-gray-700">{d.owner_name || '未设负责人'}</span>
          </div>
          <Chip tone={d.owner_open_id ? 'green' : 'red'}>{d.owner_open_id ? '✓ 已绑定 Open ID' : '⚠ 未绑定（催办回退群 @）'}</Chip>
        </div>
      ))}
    </Section>

    <Section n="五" title="催办升级路径" tag="自动">
      <div className="flex flex-wrap items-center gap-2 text-sm">
        {['回执超时', '每 24h 私发催办 @ 未完成维度负责人（仅催 7 天内新单）', '第 3 次起追加 @ 升级提醒人', '长期无回执由安全人员直接联系', '严重/高危滞后通报管理层'].map((s, i) => (
          <span key={i} className="flex items-center gap-2">
            {i > 0 && <span className="text-gray-300">→</span>}
            <Chip tone={i >= 2 ? 'amber' : 'blue'}>{s}</Chip>
          </span>
        ))}
      </div>
    </Section>

    <Section n="六" title="重大漏洞专项群">
      <p className="text-sm text-gray-700 mb-2"><b>启动条件（满足任一）</b>：严重级且影响多产线 / 技术细节或 POC 公开 / 在野利用 / 波及自营线上 / 客户问询 / 需跨部门协作。</p>
      <div className="flex flex-wrap gap-2 text-xs">
        {['建群拉人', '发卡片 @all + 置公告', '当日影响面初判', '候选缓解方案', '组织方案同步会', '沉淀结论+通报', '按各轨执行至关闭'].map((s, i) => <Chip key={i}>{i+1}. {s}</Chip>)}
      </div>
    </Section>

    <p className="text-center text-xs text-gray-400 pb-4">依据《Vulnfeed 流程文档修订版 24 · 三维度回执规则（用户确认）》</p>
  </div>
}
