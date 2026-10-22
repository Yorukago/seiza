/* SEIZA: find out how a client was launched, so a star can offer to relaunch it. */
#ifndef SEIZA_PROCINFO_H
#define SEIZA_PROCINFO_H

#include <stddef.h>
#include <sys/types.h>

/* The command line of `pid` as one shell-safe string (arguments with spaces or
 * special characters are single-quoted). 0 ok, -1 if it can't be read or does
 * not fit in `n`. */
int procinfo_cmdline(pid_t pid, char *buf, size_t n);

#endif
