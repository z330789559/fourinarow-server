-- Migration 013: 签到连续数（驱动「香火不断」连续签到无限轮成就）
-- 在 012 的 minigame_signin_stat 上加一列 streak（当前连续签到次数）。断签由服务端 report_signin 归 1。

ALTER TABLE minigame_signin_stat ADD COLUMN streak INT NOT NULL DEFAULT 0;
