/* SEIZA: control socket, server and client. See ipc.h. */
#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <unistd.h>
#include <wlr/util/log.h>
#include "ipc.h"

#define LINE_MAX_BYTES 1024

static char sock_path[sizeof(((struct sockaddr_un *)0)->sun_path)];
static const struct cmd_ops *g_ops;
static void *g_ud;
static struct wl_event_loop *g_loop;
static struct wl_event_source *g_listen_src;
static int g_listen_fd = -1;

/* One connected client, until it sends a full line or misbehaves. */
struct conn {
	int fd;
	struct wl_event_source *src;
	char buf[LINE_MAX_BYTES];
	size_t len;
};

static void conn_close(struct conn *c) {
	wl_event_source_remove(c->src);
	close(c->fd);
	free(c);
}

static int conn_readable(int fd, uint32_t mask, void *data) {
	struct conn *c = data;
	(void)mask;
	ssize_t n = read(fd, c->buf + c->len, sizeof(c->buf) - c->len);
	if (n <= 0) { /* disconnected before sending a full line */
		if (n < 0 && (errno == EAGAIN || errno == EINTR)) {
			return 0;
		}
		conn_close(c);
		return 0;
	}
	c->len += n;
	char *nl = memchr(c->buf, '\n', c->len);
	if (!nl) {
		if (c->len == sizeof(c->buf)) { /* line too long */
			conn_close(c);
		}
		return 0;
	}
	*nl = '\0';
	char reply[160];
	cmd_run(g_ops, g_ud, c->buf, reply, sizeof(reply) - 1);
	size_t rl = strlen(reply);
	reply[rl] = '\n';
	(void)!write(fd, reply, rl + 1);
	conn_close(c);
	return 0;
}

static int accept_ready(int fd, uint32_t mask, void *data) {
	(void)mask; (void)data;
	int cfd = accept4(fd, NULL, NULL, SOCK_NONBLOCK | SOCK_CLOEXEC);
	if (cfd < 0) {
		return 0;
	}
	struct conn *c = calloc(1, sizeof(*c));
	c->fd = cfd;
	c->src = wl_event_loop_add_fd(g_loop, cfd, WL_EVENT_READABLE, conn_readable, c);
	return 0;
}

static int path_init(void) {
	const char *rt = getenv("XDG_RUNTIME_DIR");
	if (!rt || !*rt) {
		return -1;
	}
	return snprintf(sock_path, sizeof(sock_path), "%s/seiza.sock", rt) < (int)sizeof(sock_path) ? 0 : -1;
}

static int make_addr(struct sockaddr_un *a) {
	memset(a, 0, sizeof(*a));
	a->sun_family = AF_UNIX;
	strcpy(a->sun_path, sock_path);
	return 0;
}

int ipc_serve(struct wl_event_loop *loop, const struct cmd_ops *ops, void *ud) {
	if (path_init() != 0) {
		wlr_log(WLR_ERROR, "seiza: no XDG_RUNTIME_DIR, running without a control socket");
		return -1;
	}
	struct sockaddr_un addr;
	make_addr(&addr);
	int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
	if (fd < 0) {
		return -1;
	}
	if (connect(fd, (struct sockaddr *)&addr, sizeof(addr)) == 0) { /* a live owner */
		wlr_log(WLR_ERROR, "seiza: %s is in use by another compositor, running without it", sock_path);
		close(fd);
		return -1;
	}
	close(fd);
	unlink(sock_path); /* stale file from a crash */
	fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
	mode_t old = umask(0177); /* socket file 0600 */
	int rc = bind(fd, (struct sockaddr *)&addr, sizeof(addr));
	umask(old);
	if (rc != 0 || listen(fd, 8) != 0) {
		wlr_log(WLR_ERROR, "seiza: cannot listen on %s: %s", sock_path, strerror(errno));
		close(fd);
		return -1;
	}
	g_ops = ops;
	g_ud = ud;
	g_loop = loop;
	g_listen_fd = fd;
	g_listen_src = wl_event_loop_add_fd(loop, fd, WL_EVENT_READABLE, accept_ready, NULL);
	return 0;
}

void ipc_close(void) {
	if (g_listen_fd >= 0) {
		wl_event_source_remove(g_listen_src);
		close(g_listen_fd);
		unlink(sock_path);
		g_listen_fd = -1;
	}
}

int ipc_client(int argc, char **argv) {
	if (path_init() != 0) {
		fprintf(stderr, "error: XDG_RUNTIME_DIR is not set\n");
		return 1;
	}
	char line[LINE_MAX_BYTES] = "seiza";
	for (int i = 1; i < argc; i++) {
		if (strlen(line) + strlen(argv[i]) + 3 > sizeof(line)) {
			fprintf(stderr, "error: command too long\n");
			return 1;
		}
		strcat(line, " ");
		strcat(line, argv[i]);
	}
	strcat(line, "\n");
	struct sockaddr_un addr;
	make_addr(&addr);
	int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
	if (fd < 0 || connect(fd, (struct sockaddr *)&addr, sizeof(addr)) != 0) {
		fprintf(stderr, "error: seiza is not running\n");
		return 1;
	}
	if (write(fd, line, strlen(line)) < 0) {
		fprintf(stderr, "error: could not send\n");
		return 1;
	}
	char reply[256];
	ssize_t n = read(fd, reply, sizeof(reply) - 1);
	close(fd);
	if (n <= 0) {
		fprintf(stderr, "error: no reply\n");
		return 1;
	}
	reply[n] = '\0';
	fputs(reply, strncmp(reply, "ok", 2) == 0 ? stdout : stderr);
	return strncmp(reply, "ok", 2) == 0 ? 0 : 1;
}
