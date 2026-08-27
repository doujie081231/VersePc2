/* winstubs.c — 为 macOS/Linux 链接提供 Windows kernel32/shell32 的占位符号。
 *
 * 某些依赖（通过 windows-sys/winapi-util 等）在非 Windows 上仍会引用少量 Win32 API，
 * 但运行时从不会真正调用它们（对应路径会被 cfg!(windows) 短路）。这里给出 no-op 实现，
 * 使这类"死引用"在链接期得到解析，从而能正常产出 mac/Linux 二进制。
 * 返回值遵循常见错误约定：句柄/布尔失败返回无效值，取到 LAST_ERROR 语义尽量配合。
 */
#include <stdint.h>

#if defined(_WIN32) || defined(__CYGWIN__)
/* Windows 编译本文件不应发生；此处为空实现以防万一 */
#else

typedef uintptr_t HANDLE;
#define INVALID ((HANDLE)~0ULL)

/* --- kernel32 —— 文件/管道/进程/令牌 --- */
HANDLE CreateFileW(const void *file, uint32_t access, uint32_t share,
                   void *sa, uint32_t disp, uint32_t flags, void *tpl) {
    (void)file; (void)access; (void)share; (void)sa; (void)disp; (void)flags; (void)tpl;
    return INVALID;
}
int32_t CloseHandle(void *h) { (void)h; return 0; }
uint32_t ReadFile(void *h, void *buf, uint32_t n, uint32_t *read, void *ol) {
    (void)h; (void)buf; (void)n; (void)read; (void)ol; return 0;
}
uint32_t WriteFile(void *h, const void *buf, uint32_t n, uint32_t *wrote, void *ol) {
    (void)h; (void)buf; (void)n; (void)wrote; (void)ol; return 0;
}
HANDLE CreateNamedPipeW(const void *name, uint32_t openmode, uint32_t pipemode,
                        uint32_t maxinst, uint32_t outbuf, uint32_t inbuf,
                        uint32_t timeout, void *sa) {
    (void)name; (void)openmode; (void)pipemode; (void)maxinst;
    (void)outbuf; (void)inbuf; (void)timeout; (void)sa;
    return INVALID;
}
int32_t ConnectNamedPipe(HANDLE h, void *ol) { (void)h; (void)ol; return 0; }
int32_t WaitNamedPipeW(const void *name, uint32_t timeout) { (void)name; (void)timeout; return 0; }
void *GetCurrentProcess(void) { return (void*)~0ULL; }
uint32_t GetCurrentProcessId(void) { return 0; }
uint32_t GetLastError(void) { return 0; }
int32_t OpenProcessToken(void *proc, uint32_t access, void **tok) {
    (void)proc; (void)access; (void)tok; return 0;
}
int32_t GetTokenInformation(void *tok, int32_t cls, void *info, uint32_t len, uint32_t *retlen) {
    (void)tok; (void)cls; (void)info; (void)len; (void)retlen; return 0;
}
uint32_t GetModuleFileNameW(void *mod, void *buf, uint32_t len) {
    (void)mod; (void)buf; (void)len; return 0;
}

/* --- shell32 --- */
void *ShellExecuteExW(void *info) { (void)info; return (void*)0; }

#endif /* non-Windows guards */