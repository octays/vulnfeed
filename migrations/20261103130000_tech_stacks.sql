-- Tech-stack dictionary (admin maintained) + stack tagging on items and
-- subscriptions on channels/chats. Empty subscription = receive everything.
CREATE TABLE tech_stack (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    slug TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL DEFAULT '',
    keywords TEXT NOT NULL DEFAULT '',
    enabled BOOLEAN NOT NULL DEFAULT TRUE
);
INSERT INTO tech_stack(slug,name,keywords) VALUES
 ('php','PHP','php,laravel,thinkphp,wordpress,composer,discuz,phpunit,drupal,joomla,zend'),
 ('go','Go','golang,go语言,go 1.,gin-gonic,gorilla,k8s.io,containerd,runc,cobra,gofiber'),
 ('rust','Rust','rust,cargo,crates.io,serde,tokio,rustc'),
 ('java','Java','java,spring,tomcat,log4j,fastjson,weblogic,jboss,shiro,jackson,struts,kafka,elastic'),
 ('python','Python','python,django,flask,pip,pypi,requests,pytorch'),
 ('node','Node.js','node.js,nodejs,npm,yarn,electron,vue,react,webpack,next.js,deno'),
 ('linux','Linux','linux kernel,linux,ubuntu,centos,debian,red hat,grub,systemd,sudo,curl,wget,openssh,bash'),
 ('windows','Windows','windows,microsoft,exchange,iis,powershell,active directory,office,rdp'),
 ('k8s','Kubernetes','kubernetes,k8s,kubectl,docker,containerd,openshift,rancher'),
 ('db','数据库','mysql,postgres,postgresql,redis,mongodb,elasticsearch,oracle,mariadb,sqlite,clickhouse');
ALTER TABLE vuln_information ADD COLUMN stacks TEXT NOT NULL DEFAULT '';
ALTER TABLE security_notice ADD COLUMN stacks TEXT NOT NULL DEFAULT '';
ALTER TABLE push_channel ADD COLUMN stacks TEXT NOT NULL DEFAULT '';
ALTER TABLE feishu_chat ADD COLUMN stacks TEXT NOT NULL DEFAULT '';
