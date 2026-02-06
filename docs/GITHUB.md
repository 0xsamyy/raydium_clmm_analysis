# GitHub Setup (WSL + SSH)

## 1. Create the Repository on GitHub

Create a new repository in the GitHub web UI. Do not initialize it with a README or .gitignore (this repo already has them).

## 2. Verify SSH Access

From WSL, run:

```
ssh -T git@github.com
```

If this fails, add an SSH key to your GitHub account and retry.

## 3. Initialize Git Locally (if needed)

From the repo root:

```
git init
git status
```

## 4. Commit the Current State

```
git add .
git commit -m "Initial commit"
```

## 5. Add the Remote and Push

Replace `<USER>` and `<REPO>` with your GitHub values:

```
git remote add origin git@github.com:<USER>/<REPO>.git
git branch -M main
git push -u origin main
```

## 6. Verify

Refresh the repository page on GitHub to confirm all files are present.
