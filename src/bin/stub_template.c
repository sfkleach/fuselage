#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

// Baked-in fuselage argument list, substituted by fuselage-pack at pack time.
// FUSELAGE_PACK_ARGS is replaced with a C array body such as:
//   "--static=/run/fuselage/myapp:/proc/self/exe", "--run", "python", "--", "-m", "myapp"
static const char *baked_args[] = {
    FUSELAGE_PACK_ARGS
    NULL
};

// Count elements in baked_args (excluding the NULL terminator).
static int baked_argc(void) {
    int n = 0;
    while (baked_args[n] != NULL) n++;
    return n;
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
    // fuselage baked_args[0..n_baked] -- self_path argv[1..argc-1]
    // Total: 1 (fuselage) + n_baked + 1 (--) + (argc-1) (user args) + 1 (NULL)
    int n_user = argc - 1;
    int total = 1 + n_baked + 1 + n_user + 1;
    const char **new_argv = malloc(total * sizeof(char *));
    if (new_argv == NULL) {
        fprintf(stderr, "fuselage stub: out of memory\n");
        return 1;
    }

    int i = 0;
    new_argv[i++] = "fuselage";
    for (int j = 0; j < n_baked; j++) {
        // Substitute the literal token /proc/self/exe with the resolved path.
        if (strcmp(baked_args[j], "/proc/self/exe") == 0) {
            new_argv[i++] = self_path;
        } else {
            new_argv[i++] = baked_args[j];
        }
    }
    new_argv[i++] = "--";
    for (int j = 1; j < argc; j++) {
        new_argv[i++] = argv[j];
    }
    new_argv[i] = NULL;

    execvp("fuselage", (char *const *)new_argv);
    fprintf(stderr, "fuselage stub: exec fuselage: %s\n", strerror(errno));
    return 127;
}
