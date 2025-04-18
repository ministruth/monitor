export default {
  'pages.config.content': '配置插件 monitor',

  'pages.config.setting.title': '服务设置',
  'pages.config.setting.status.text': '服务状态：',
  'pages.config.setting.status.running': '正在运行',
  'pages.config.setting.status.stopped': '已停止',
  'pages.config.setting.status.stopped.title': '停止 monitor 服务器，确认？',
  'pages.config.setting.status.stopped.content': '所有客户端将会立即断开连接。',
  'pages.config.setting.address.text': '监听地址：',
  'pages.config.setting.shell.text': '终端：',
  'pages.config.setting.shell.new': '新终端',
  'pages.config.setting.cert.text': '证书：',
  'pages.config.setting.cert.get': '下载公钥文件',
  'pages.config.setting.cert.regenerate': '重新生成',
  'pages.config.setting.cert.regenerate.title': '重新生成公私钥文件，确认？',
  'pages.config.setting.cert.regenerate.content':
    '所有客户端将会被踢出，需要新公钥才能连接。',
  'pages.config.setting.msg.timeout.text': '消息超时',
  'pages.config.setting.msg.timeout.tip':
    '接收消息的超时时间（秒），为0时禁用。注意当agent回报率比该值长时连接可能会被关闭。',
  'pages.config.setting.alert.timeout.text': '告警超时：',
  'pages.config.setting.alert.timeout.tip':
    '发送agent离线告警前的超时时间（秒），为0时禁用。',
  'pages.config.setting.timeout.second': '秒',

  'pages.config.agent.title': '客户端设置',
  'pages.config.agent.reconnect.tip': '重连',
  'pages.config.agent.reconnect.title': '重新连接客户端 {name}，确认？',
  'pages.config.agent.update.title': '更新客户端',
  'pages.config.agent.form.name.tip': '客户端唯一名称',
  'pages.config.agent.delete.title': '删除客户端 {name}，确认？',
  'pages.config.agent.delete.selected.title': '删除选中的客户端，确认？',
  'pages.config.agent.passive': '被动客户端',
  'pages.config.agent.passive.title': '管理被动客户端',
  'pages.config.agent.activate.passive.tip': '激活',
  'pages.config.agent.add.passive.title': '添加被动客户端',
  'pages.config.agent.update.passive.title': '更新被动客户端',
  'pages.config.agent.delete.passive.selected.title':
    '删除选中的被动客户端，确认？',
  'pages.config.agent.delete.passive.title': '删除被动客户端 {name}，确认？',

  'pages.view.content': '客户端监控',
  'pages.view.card.agent': '客户端',
  'pages.view.card.shell.text': '终端：',
  'pages.view.card.shell.placeholder': '加载终端列表中',
  'pages.view.card.connect.text': '连接',
  'pages.view.card.reconnect.text': '重连',
  'pages.view.card.reconnect.title': '重新连接，确认？',
  'pages.view.card.reconnect.content': '当前连接将会丢失！',
  'pages.view.card.shell.tip': '终端',
};
