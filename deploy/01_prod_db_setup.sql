-- =============================================================================
-- fourinarow — 生产环境数据库 + 账号初始化脚本
-- 在【生产 PostgreSQL 服务器】上以超级用户 (postgres) 身份执行一次。
-- 用法：  psql -U postgres -h <PROD_HOST> -f 01_prod_db_setup.sql
--
-- ⚠️ 执行前请把下面的 :'app_pass' 占位密码改成强密码（或用 -v 传入，见文件末尾）。
-- =============================================================================

\set ON_ERROR_STOP on

-- 可在命令行覆盖： psql -v app_user=fourinarow_app -v app_pass='S3cr3t' -v db_name=fourinarow ...
\if :{?app_user} \else \set app_user 'fourinarow_app' \endif
\if :{?app_pass} \else \set app_pass 'fourinarow_app' \endif
\if :{?db_name}  \else \set db_name  'fourinarow' \endif

-- ── 1. 创建专用应用账号（角色） ────────────────────────────────────────────────
-- 已存在则改密码，不存在则创建。LOGIN = 可登录账号。
-- 注意：psql 的 :'var' 变量插值【不会】在 $$...$$ dollar 引号内生效（psql 把整个
-- 块当字符串原样发给服务端，: 不替换 → 语法错），所以这里不能用 DO 块。
-- 改用 format()+\gexec（与下方 CREATE DATABASE 同款）：format 在 $$ 之外，
-- :'app_user' / :'app_pass' 能正常替换；\gexec 再执行拼好的 DDL。
SELECT format('CREATE ROLE %I WITH LOGIN PASSWORD %L', :'app_user', :'app_pass')
WHERE NOT EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'app_user')
\gexec
SELECT format('ALTER ROLE %I WITH LOGIN PASSWORD %L', :'app_user', :'app_pass')
WHERE EXISTS (SELECT 1 FROM pg_roles WHERE rolname = :'app_user')
\gexec

-- ── 2. 创建专用数据库，属主 = 应用账号 ─────────────────────────────────────────
-- CREATE DATABASE 不能放进事务/DO 块，所以用 \gexec 动态执行；已存在则跳过。
SELECT format('CREATE DATABASE %I OWNER %I ENCODING ''UTF8'' TEMPLATE template0', :'db_name', :'app_user')
WHERE NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = :'db_name')
\gexec

-- ── 3. 把整个数据库的所有权限授予应用账号 ──────────────────────────────────────
SELECT format('GRANT ALL PRIVILEGES ON DATABASE %I TO %I', :'db_name', :'app_user') \gexec

-- ── 4. 切到该库，处理 public schema 权限（PG15+ 默认不再对 PUBLIC 开放 CREATE） ──
\connect :db_name

-- 让应用账号拥有 public schema，后续 pg_restore 用该账号导入时所有对象都归它所有。
ALTER SCHEMA public OWNER TO :"app_user";
GRANT ALL ON SCHEMA public TO :"app_user";

-- 对“将来”由其他角色（如 postgres）创建的对象，自动给应用账号全部权限（保险起见）。
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON TABLES    TO :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON SEQUENCES TO :"app_user";
ALTER DEFAULT PRIVILEGES IN SCHEMA public GRANT ALL ON FUNCTIONS TO :"app_user";

\echo '✅ 数据库与账号初始化完成。接下来用应用账号导入 dump（见 03_restore.sh）。'
