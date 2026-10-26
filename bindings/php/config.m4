PHP_ARG_WITH([sortition],
  [for libsortition support],
  [AS_HELP_STRING([--with-sortition@<:@=DIR@:>@],
    [DIR holds include/sortition.h and lib/libsortition.so (default: this repo's core/include and target/release)])])

if test "$PHP_SORTITION" != "no"; then
  if test "$PHP_SORTITION" = "yes"; then
    SORTITION_ROOT=`cd "$srcdir/../.." && pwd`
    SORTITION_INC="$SORTITION_ROOT/core/include"
    SORTITION_LIB="$SORTITION_ROOT/target/release"
  else
    SORTITION_INC="$PHP_SORTITION/include"
    SORTITION_LIB="$PHP_SORTITION/lib"
  fi

  if test ! -r "$SORTITION_INC/sortition.h"; then
    AC_MSG_ERROR([sortition.h not found in $SORTITION_INC])
  fi

  PHP_ADD_INCLUDE([$SORTITION_INC])
  PHP_ADD_LIBRARY_WITH_PATH([sortition], [$SORTITION_LIB], [SORTITION_SHARED_LIBADD])
  dnl 実行時に libsortition.so を見つけられるよう rpath を埋め込む
  SORTITION_SHARED_LIBADD="-Wl,-rpath,$SORTITION_LIB $SORTITION_SHARED_LIBADD"
  PHP_SUBST([SORTITION_SHARED_LIBADD])

  PHP_NEW_EXTENSION([sortition], [sortition.c], [$ext_shared])
fi
