const messages = [
  '牛马起来干活了',
  '服务器都醒了 你还在等什么',
  '键盘一响 黄金万两',
  '咖啡已续命 终端请就位',
  '今天也要优雅地假装很忙',
  '工位已就绪 灵魂稍后上线',
  '老板画的饼 服务器来烤',
  '把班上明白 把鱼摸漂亮',
  '摸鱼可以 连接别断',
  '打工人的一天 从连上服务器开始',
  '人可以困 终端不能掉线',
  '今天的工位由你承包',
  '别看了 服务器在等你翻牌',
  '你负责敲键盘 我负责装专业',
  '生活需要回车 工作需要撤回',
  '先连上再说 万一今天很顺呢',
  '今天的目标是准点下班',
  '代码还没醒 先让终端热个身',
  '上班的仪式感 从打开终端开始',
  '不想上班 但想把事情搞定',
  '今日份的赛博搬砖开始了',
  '键盘已到位 就差你点头',
  '打工可以 灵魂记得下班',
  '别急着开卷 先把连接开了',
  '世界那么大 工单那么多',
  '昨天的坑 今天轻轻填',
  '今天争取少看两眼日志',
  '一顿操作猛如虎 下班速度也要有',
  '先干一小会 再奖励自己一小会',
  '连接的是主机 盼着的是下班',
  '服务器没有周一综合征 但我有',
  '今天也在为下班认真准备',
  '把问题留在终端 把快乐带回家',
  '工作可以慢慢来 下班必须准时',
  '今天少一点报错 多一点好运',
  '终端已经打开 好日子还会远吗'
];

export function getDailyWelcomeMessage(date = new Date()) {
  // Local calendar dates keep the same message across restarts without storing UI state.
  const day = `${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
  let hash = 2166136261;
  for (const character of day) {
    hash = Math.imul(hash ^ character.charCodeAt(0), 16777619);
  }
  return messages[(hash >>> 0) % messages.length];
}
