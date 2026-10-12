# Legacy source provenance

Reference: https://gitlab.com/eiscat/guisdap9.git

Pinned commit: `a9c4415ab6564e6a154152ca3ac569ee442a658f`.

```sh
mkdir -p legacy
GIT_LFS_SKIP_SMUDGE=1 git clone https://gitlab.com/eiscat/guisdap9.git legacy/guisdap9
git -C legacy/guisdap9 checkout a9c4415ab6564e6a154152ca3ac569ee442a658f
```

The first checkout is shallow at this commit. LFS smudging is disabled; retrieve any
needed LFS assets explicitly. The inspected BEATA sources and initialization files
are present. This directory is a separate reference repository, not a submodule and
not part of the new repository's commits.

Relevant references: `exps/beata/pl_def.m`, `beataT_2_LP.m`, `init/COR_fdalt.m`,
`source/plwin.txt`, `source/plwin.c`, `anal/plasma_summary.m`, `anal/forward_spec.m`.
We also inspected the existing EISCAT RTG `fd_alt.m`, `pp.m` and `acf2spec.m`
on revontuli and the observation's `rtg_def_u.m` and `beata-u.fil`.

EISCAT's current release page links the newer upstream at
https://gitlab.com/ingemarh/guisdap. This first implementation uses the EISCAT
GUISDAP9 reference requested for legacy comparison, including its BEATA 2.2 layout.

Public documentation:
https://eiscat.se/technology/user-documents/guisdap/
