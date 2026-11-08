#include "policy.h"
#include <assert.h>
#include <stdio.h>
#include <string.h>
typedef struct { char steps[16]; unsigned used; int result; } Trace;
static void append(Trace *t, char c) { t->steps[t->used++] = c; }
static int requirement(void *p) { Trace *t=p; append(t,'R'); return t->result; }
static void retire(void *p) { append(p,'T'); }
static void cancel(void *p) { append(p,'C'); }
static void activate(void *p, bool cleanup) { append(p,cleanup?'D':'A'); }
int main(void) {
    Trace failed={.result=1};
    ProbePolicy p={&failed,requirement,retire,cancel,activate};
    assert(!probe_configure(p,true)); assert(!strcmp(failed.steps,"RTCD"));
    Trace borrowed={.result=1}; p.context=&borrowed;
    assert(!probe_configure(p,false)); assert(!strcmp(borrowed.steps,"RTC"));
    Trace passed={0}; p.context=&passed;
    assert(probe_configure(p,true)); assert(!strcmp(passed.steps,"RA"));
    puts("policy: 3 passed (injected operations, no connections)");
}
