#!/usr/bin/env bash
# 在【生产】导入 dump。前置：已先跑过 01_prod_db_setup.sql 建好库和账号。
# 用应用账号导入 -> 所有表/序列/索引的属主都会是应用账号。
set -euo pipefail

PROD_HOST=${PROD_HOST:-127.0.0.1}
PROD_PORT=${PROD_PORT:-5432}
APP_USER=${APP_USER:-fourinarow_app}
DB=${DB:-fourinarow}
DUMP=${1:?用法: PGPASSWORD=应用密码 ./03_restore.sh <fourinarow_xxx.dump|.sql>}

export PGPASSWORD="${PGPASSWORD:?请用 PGPASSWORD 环境变量传应用账号密码}"

case "$DUMP" in
  *.dump)   # 自定义格式 -> pg_restore，可并行
    pg_restore -h "$PROD_HOST" -p "$PROD_PORT" -U "$APP_USER" -d "$DB" \
      --no-owner --no-privileges --jobs=4 "$DUMP"
    ;;
  *.sql)    # 纯 SQL -> psql
    psql -h "$PROD_HOST" -p "$PROD_PORT" -U "$APP_USER" -d "$DB" \
      -v ON_ERROR_STOP=1 -f "$DUMP"
    ;;
  *) echo "未知格式: $DUMP"; exit 1 ;;
esac

echo "✅ 导入完成。抽查行数："
psql -h "$PROD_HOST" -p "$PROD_PORT" -U "$APP_USER" -d "$DB" -tAc \
  "SELECT 'users='||count(*) FROM users;"
