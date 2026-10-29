# Edge cases

## Transcript: only `$` lines run

Run should execute 2 commands (not the output lines). Copy copies everything.

```console
$ mkdir -p /tmp/sawit-fixture && cd /tmp/sawit-fixture
$ echo "transcript ran in $(pwd)"
transcript ran in /tmp/sawit-fixture
```

Indented prompts and a bare `$` line:

```bash
  $ echo "indented prompt"
indented prompt
$
$ echo "after bare prompt"
```

## No prompts: every line runs

```bash
echo "line 1"
echo "line 2"
echo "line 3"
```

## Line continuation

```bash
echo "one" \
  "two" \
  "three"
```

## Heredoc

```bash
mkdir -p /tmp/sawit-fixture
cat > /tmp/sawit-fixture/hello.txt <<'TXT'
hello from a heredoc
second line
TXT
cat /tmp/sawit-fixture/hello.txt
```

## Quotes, variables, unicode

```zsh
name="Sawit 🌴"
echo "Hello, $name — it's \"quoted\" and 'single' too"
echo 'no $expansion here'
```

## `$` that is not a prompt

Should run as-is (no line starts with `$ `):

```bash
echo "$HOME" && echo "${SHELL:-unknown}"
```

## Long first line (buttons must not cover it)

```bash
echo "This first line is deliberately very long so that it reaches the right edge of the preview where the Run and Copy buttons sit — they must not cover it" && echo done
```

## Leading and trailing blank lines

```bash


echo "blank lines around me are trimmed"


```

## Comments only

```bash
# just a comment, Run should do nothing harmful
```

## Language names

Uppercase and extra spaces still count as shell:

```BASH
echo "OK uppercase BASH"
```

```Zsh
echo "OK mixed-case Zsh"
```

No language: **Copy only**, no label:

```
echo "no language: no Run button"
```

Unknown language: **Copy only**:

```bashx
echo "bashx is not a shell"
```

## Nested blocks

1. In a list item:

   ```bash
   echo "OK nested in list"
   ```

> In a blockquote:
>
> ```sh
> echo "OK nested in blockquote"
> ```

- Deeper:
  - Second level:

    ```zsh
    echo "OK second-level list"
    ```

## Long-running and interactive

Stop it with Ctrl-C in the terminal:

```bash
for i in 1 2 3 4 5; do echo "tick $i"; sleep 1; done
```

## Tall block (scrolling)

```bash
echo 01
echo 02
echo 03
echo 04
echo 05
echo 06
echo 07
echo 08
echo 09
echo 10
echo 11
echo 12
echo 13
echo 14
echo 15
echo 16
echo 17
echo 18
echo 19
echo 20
echo 21
echo 22
echo 23
echo 24
echo 25
echo 26
echo 27
echo 28
echo 29
echo 30
```

## Cleanup

```bash
rm -rf /tmp/sawit-fixture && echo "cleaned up"
```
