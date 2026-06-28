# 生产数据库初始化与数据迁移

把本地 `fourinarow` 库（表/数据/序列/索引/约束/种子数据）迁移到生产 PostgreSQL。

## 文件
| 文件 | 在哪执行 | 作用 |
|------|----------|------|
| `01_prod_db_setup.sql` | 生产，超级用户 `postgres` | 建专用账号、建专用库（属主=应用账号）、整库授权、处理 PG15+ 的 `public` schema 权限 |
| `02_export.sh` | 本地 | 用 `pg_dump` 从 docker 容器 `pg` 导出全库（custom + plain 两种格式） |
| `03_restore.sh` | 生产 | 用应用账号把 dump 导入，导入后所有对象归应用账号所有 |

> ⚠️ 导出的 dump 文件含真实用户 PII（邮箱、密码哈希、平台 session_key、登录 token），
> **不要提交进 git**。已在 `.gitignore` 屏蔽 `*.dump` / `fourinarow_*.sql` / `deploy/dumps/`。

## 步骤

```bash
# 1) 生产建库建号（先把脚本里的密码改成强密码，或用 -v 传入）
psql -U postgres -h <PROD_HOST> \
  -v app_user=fourinarow_app -v app_pass='强密码' -v db_name=fourinarow \
  -f deploy/01_prod_db_setup.sql

# 2) 本地导出（输出到 deploy/dumps/，该目录已被 gitignore）
mkdir -p deploy/dumps
./deploy/02_export.sh deploy/dumps

# 3) 生产导入（用应用账号）
PGPASSWORD='强密码' ./deploy/03_restore.sh deploy/dumps/fourinarow_<时间戳>.dump
```

## 说明
- 本地连接：docker 容器 `pg`，超级用户 `postgres`（非 docker-compose 里的 `fourinarow:fourinarow`）。
- `pg_dump` 自动包含**序列定义 + `setval` 当前自增值、所有索引、主外键/唯一/CHECK 约束**。
- `--no-owner --no-privileges`：剥离原属主 `postgres`，导入时归还给执行账号（应用账号）。
- `_sqlx_migrations` 表会一并迁移，导入后 sqlx 不会重跑迁移——保留它。
- `sessions` 是临时登录态，可在 `02_export.sh` 加 `--exclude-table-data=sessions` 不迁移。
- 应用侧 `DATABASE_URL` 改为：`postgres://fourinarow_app:密码@<PROD_HOST>:5432/fourinarow`
