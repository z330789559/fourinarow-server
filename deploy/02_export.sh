#!/usr/bin/env bash
# 在【本地】导出 fourinarow 全库（表/数据/序列当前值/索引/约束/种子数据）。
# 本地 PostgreSQL 跑在 docker 容器 `pg` 里，超级用户 postgres。
set -euo pipefail

DB=fourinarow
STAMP=$(date +%Y%m%d_%H%M%S)
OUT_DIR=${1:-.}

# --no-owner / --no-privileges：去掉原属主(postgres)和 ACL，导入时归还给执行账号(应用账号)。
# 序列、索引、约束、setval(当前自增值) 都由 pg_dump 自动包含，无需额外处理。
COMMON="--no-owner --no-privileges --encoding=UTF8"

# A) 自定义压缩格式（推荐用于生产导入：体积小，可并行/选择性还原）
docker exec pg pg_dump -U postgres -d "$DB" $COMMON --format=custom \
  > "$OUT_DIR/${DB}_${STAMP}.dump"

# B) 纯 SQL 文本（可读、可直接 psql 导入、便于 review/diff）
docker exec pg pg_dump -U postgres -d "$DB" $COMMON --format=plain \
  > "$OUT_DIR/${DB}_${STAMP}.sql"

echo "导出完成："
ls -lh "$OUT_DIR/${DB}_${STAMP}".dump "$OUT_DIR/${DB}_${STAMP}".sql

# 可选：会话 token 表 sessions 是临时登录态，迁移后用户会重新登录，可不导。
# 若想排除：在 COMMON 后加  --exclude-table-data=sessions
