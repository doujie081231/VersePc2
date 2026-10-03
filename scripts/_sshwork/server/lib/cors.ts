// 反馈接口 CORS：仅给启动器/官网反馈相关接口放行跨域读取
export const CORS_ALLOW_ALL = {
  "Access-Control-Allow-Origin": "*",
  "Access-Control-Allow-Methods": "GET,POST,OPTIONS",
  "Access-Control-Allow-Headers": "Content-Type",
  "Access-Control-Max-Age": "86400",
};