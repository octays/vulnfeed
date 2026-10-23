# VulnFeed 高价值漏洞、安全公告采集与推送

VulnFeed 是一个用于收集和推送高价值漏洞和补丁公告信息的工具。它从多个数据源抓取漏洞和公告信息，并通过钉钉机器人进行推送。

## 数据源

当前抓取漏洞来源的站点的数据:

| 名称                 | 地址                                                           | 推送策略                                                                     |
| -------------------- | -------------------------------------------------------------- | ---------------------------------------------------------------------------- |
| 阿里云漏洞库         | <https://avd.aliyun.com/high-risk/list>                        | 等级为高危或严重                                                             |
| OSCS开源安全情报预警 | <https://www.oscs1024.com/cm>                                  | 等级为高危或严重**并且**包含 `预警` 标签                                     |
| 知道创宇Seebug漏洞库 | <https://www.seebug.org/>                                      | 等级为高危或严重                                                             |
| CISA KEV             | <https://www.cisa.gov/known-exploited-vulnerabilities-catalog> | 全部推送                                                                     |
| 奇安信威胁情报中心   | <https://ti.qianxin.com/>                                      | 等级为高危严重**并且**包含 `奇安信CERT验证` `POC公开` `技术细节公布`标签之一 |
| 微步在线             | <https://x.threatbook.com/v5/vulIntelligence>                  | 等级为高危或严重                                                             |

当抓取安全补丁公告的站点数据：

| 名称                  | 地址                                                                                                 | 推送策略                                                                         |
| --------------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------- |
| 用友安全中心          | <https://security.yonyou.com/#/home>                                                                 | 近三条数据                                                                       |
| 泛微ECOLOGY安全补丁包 | <https://www.weaver.com.cn/cs/securityDownload.html?src=cn>                                          | "EC9.0全量补丁", "EC8.0全量补丁", "EC10.0安全补丁"                               |
| Smartbi安全补丁包     | <https://www.smartbi.com.cn/patchinfo>                                                               | 近三条数据                                                                       |
| 帆软安全漏洞声明      | <https://help.fanruan.com/finereport/doc-view-4833.html>                                             | 近三条数据                                                                       |
| 致远安全补丁          | <https://service.seeyon.com/patchtools/tp.html#/patchList?type=%E5%AE%89%E5%85%A8%E8%A1%A5%E4%B8%81> | V5的近三条数据                                                                   |
| Vmware安全公告        | <https://support.broadcom.com/web/ecx/security-advisory?>                                            | 近十条数据                                                                       |
| Oracle安全公告        | <https://www.oracle.com/cn/security-alerts/>                                                         | 近三条数据                                                                       |
| Firefox安全公告       | <https://www.mozilla.org/en-US/security/known-vulnerabilities/firefox/>                              | 近三条数据                                                                       |
| Apple安全公告         | <https://support.apple.com/zh-cn/100100>                                                             | 近十条数，不包含没有链接的据                                                     |
| Grafana安全公告       | <https://grafana.com/tags/security/>                                                                 | 第一页数据中以"Grafana security release" 或 "Grafana security update" 开头的公告 |
| 深信服安全公告        | <https://www.sangfor.com.cn/sec_center/bulletins>                                                    | 第一页数据前三条数据                                                             |
| 东方通服安全公告      | <https://www.tongtech.com/dft/download.html>                                                         | 前三条数据                                                                       |

![app](./images/app.jpg)

![detail](./images/detail.jpg)

![plugin](./images/plugin.jpg)

![vuln_ding](./images/vuln_ding.jpg)

![notice_ding](./images/notice_ding.jpg)

## Features

- [x] 支持多数据源采集
- [x] 支持定时任务
- [x] 支持钉钉推送
- [x] 支持根据 CVE 号在`github`上检索近一年的相关代码仓库链接
- [x] 支持根据 CVE 号在 `nuclei` PR 记录检索相关记录链接
- [x] 支持重安全软件安全补丁公告

## 本地开发

数据库使用 SQLite，无需单独部署数据库服务，数据表结构在服务启动时自动迁移。

前端静态资源在 `cargo build` 时自动内嵌进二进制文件：构建脚本检测到 `assets/public` 不存在时会自动调用 `bun` 构建前端（需要安装 bun；未安装时跳过并输出警告，可用 `VULNFEED_SKIP_FRONTEND_BUILD=1` 强制跳过）。

### 后端服务

创建初始化管理员密码

```bash
cargo run --bin vulnfeed create-super-user -c ./dev/config.toml -p "密码"
```

```bash
cargo run --bin vulnfeed server -c ./dev/config.toml
```

### 前端服务

```bash
cd assets && bun install
bun dev
```

## 服务部署

项目已经写好了 `Dockerfile` 和 `docker-compose.yml`, 可以本地构建服务镜像，这里将前端代码打包到后端服务中，不需要单独的Nginx服务，也不需要数据库服务（SQLite 数据文件挂载在 `./data` 目录）。

```bash
docker build -t vulnfeed:latest .
docker save -o vulnfeed.tar vulnfeed:latest
```

将构建好的镜像在服务器上导入。

```bash
docker load -i vulnfeed.tar
```

服务器创建部署目录：

```bash
mkdir /data/vulnfeed/config /data/vulnfeed/data -p
```

将 `dev/config.toml` 配置文件拷贝到 `/data/vulnfeed/config`

将 `docker-compose.yml` 拷贝到 `/data/vulnfeed/`

```bash
docker-compose up -d
docker exec CONTAINER_NAME /app/vulnfeed create-super-user -c /app/config.toml -p "密码"
```
