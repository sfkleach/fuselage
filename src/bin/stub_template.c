#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Baked-in fuselage argument list, substituted by fuselage-bundle at pack time.
// FUSELAGE_BUNDLE_ARGS is replaced with a C array body such as:
//   "--static=/run/fuselage/myapp:/proc/self/exe", "--run", "python", "--", "-m", "myapp"
static const char *baked_args[] = {
    FUSELAGE_BUNDLE_ARGS
    NULL
};

// Count elements in baked_args (excluding the NULL terminator).
static int baked_argc(void) {
    int n = 0;
    while (baked_args[n] != NULL) n++;
    return n;
}

// Replace every occurrence of the literal token "/proc/self/exe" within `arg`
// with `self_path`. The token usually appears embedded in a larger argument,
// e.g. "--static=/run/fuselage/myapp:/proc/self/exe", so a whole-string compare
// is not enough — a substring replacement is required.
//
// Returns `arg` unchanged when the token is absent (no allocation in the common
// case), otherwise a newly allocated string. The allocation is never freed: the
// process either execs away or exits immediately afterwards.
static const char *substitute_self_path(const char *arg, const char *self_path) {
    static const char token[] = "/proc/self/exe";
    const size_t token_len = sizeof(token) - 1;

    int count = 0;
    for (const char *p = strstr(arg, token); p != NULL; p = strstr(p + token_len, token)) {
        count++;
    }
    if (count == 0) {
        return arg;
    }

    const size_t self_len = strlen(self_path);
    // Upper bound on the result length: each occurrence contributes at most
    // self_len bytes in place of the token. Over-allocating by the token bytes
    // we drop avoids any risk of unsigned underflow when self_len < token_len.
    char *out = malloc(strlen(arg) + (size_t)count * self_len + 1);
    if (out == NULL) {
        fprintf(stderr, "fuselage stub: out of memory\n");
        exit(1);
    }

    char *dst = out;
    const char *src = arg;
    for (const char *p = strstr(src, token); p != NULL; p = strstr(src, token)) {
        size_t prefix = (size_t)(p - src);
        memcpy(dst, src, prefix);
        dst += prefix;
        memcpy(dst, self_path, self_len);
        dst += self_len;
        src = p + token_len;
    }
    strcpy(dst, src); // Copy the trailing remainder, including the NUL.
    return out;
}

int main(int argc, char *argv[]) {
    // Resolve the absolute path of this binary via /proc/self/exe.
    char self_path[4096];
    ssize_t len = readlink("/proc/self/exe", self_path, sizeof(self_path) - 1);
    if (len < 0) {
        fprintf(stderr, "fuselage stub: readlink /proc/self/exe: %s\n", strerror(errno));
        return 1;
    }
    self_path[len] = '\0';

    int n_baked = baked_argc();

    // Check whether baked_args already contains a "--" separator. If it does,
    // the user's runtime args slot in after the baked args with no extra "--":
    //   fuselage [opts] -- [baked subcommand args] [user args]
    // If not, we add "--" ourselves so that user args starting with "-" are not
    // misinterpreted as fuselage flags:
    //   fuselage [opts] [baked opts] -- [user args]
    int baked_has_dashdash = 0;
    for (int j = 0; j < n_baked; j++) {
        if (strcmp(baked_args[j], "--") == 0) {
            baked_has_dashdash = 1;
            break;
        }
    }

    // Total slots: 1 (fuselage) + n_baked + (0 or 1 for "--") + n_user + 1 (NULL)
    int n_user = argc - 1;
    int total = 1 + n_baked + (baked_has_dashdash ? 0 : 1) + n_user + 1;
    const char **new_argv = malloc(total * sizeof(char *));
    if (new_argv == NULL) {
        fprintf(stderr, "fuselage stub: out of memory\n");
        return 1;
    }

    int i = 0;
    new_argv[i++] = "fuselage";
    for (int j = 0; j < n_baked; j++) {
        // Substitute the /proc/self/exe token (possibly embedded in a larger
        // argument) with this binary's resolved path.
        new_argv[i++] = substitute_self_path(baked_args[j], self_path);
    }
    if (!baked_has_dashdash) {
        new_argv[i++] = "--";
    }
    for (int j = 1; j < argc; j++) {
        new_argv[i++] = argv[j];
    }
    new_argv[i] = NULL;

    execvp("fuselage", (char *const *)new_argv);
    fprintf(stderr, "fuselage stub: exec fuselage: %s\n", strerror(errno));
    return 127;
}
