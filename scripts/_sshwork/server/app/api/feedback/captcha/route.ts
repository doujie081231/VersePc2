import { NextResponse } from "next/server";
import { captchaSvg, createCaptcha } from "@/lib/captcha";
import { CORS_ALLOW_ALL } from "@/lib/cors";

export async function OPTIONS() {
  return new NextResponse(null, { status: 204, headers: CORS_ALLOW_ALL });
}

export async function GET() {
  const captcha = await createCaptcha();
  return NextResponse.json(
    { id: captcha.id, svg: captchaSvg(captcha.code) },
    { headers: CORS_ALLOW_ALL }
  );
}