# Kronroe Logo System Deployment — Prompt for Claude Code

## Goal

Install the new graph-triangle logo system, replacing the old bridge logo on the site.

## Step 1: Create the logo directory

```bash
mkdir -p ~/kronroe/design/branding/logo
```

## Step 2: Copy all logo SVG files

Copy every `.svg` and `.md` file from `~/kronroe/design/branding/logo-staging/` into `~/kronroe/design/branding/logo/`.

The files to create are listed in `LOGO-SYSTEM.md` in the staging directory. Read that file for the exact SVG content of each file.

If the staging directory doesn't exist, the SVG files are documented in the LOGO-SYSTEM.md file — create them from the specifications there.

## Step 3: Deploy to the live site

```bash
# Nav bar logo (dark variant for black header)
cp ~/kronroe/design/branding/logo/kronroe-mark-dark.svg ~/kronroe/site/public/logo-v3x6-violet.svg

# Favicon
cp ~/kronroe/design/branding/logo/kronroe-favicon.svg ~/kronroe/site/public/favicon.svg

# Contained mark (for GitHub avatar etc)  
cp ~/kronroe/design/branding/logo/kronroe-mark-contained-purple.svg ~/kronroe/site/public/logo-contained-dark.svg

# Also copy the light mark
cp ~/kronroe/design/branding/logo/kronroe-mark-light.svg ~/kronroe/site/public/kronroe-logo.svg
```

## Step 4: Update index.html

Find the logo `<img>` tag in the header and ensure:
- It references `/logo-v3x6-violet.svg`
- Height is `36` (or adjust to `40` if it looks too small in the nav)

```html
<img src="/logo-v3x6-violet.svg" alt="Kronroe" height="36" style="display:block;" />
```

Also update the CSS `.logo img` height to match:
```css
.logo img {
  display: block;
  height: 36px;
  width: auto;
}
```

## Step 5: Commit and deploy

```bash
cd ~/kronroe
git add design/branding/logo/ site/public/
git commit -m "feat(brand): new graph-triangle logo system — violet/orange/lime three-node mark"
git push origin main
```

## Step 6: Verify

1. Check `http://localhost:5173` — logo should show three coloured dots in a V-shape
2. Check favicon in browser tab
3. After deploy, verify `https://kronroe.dev`

## Step 7: Upload GitHub avatar (manual)

Go to `github.com/kronroe` → Settings → Avatar → upload `kronroe-mark-contained-purple.svg`

---

## Reference: The logo concept

The logo is a three-node graph triangle:
- **Violet** circle (left base) = subject/entity
- **Lime** circle (right base) = value/object  
- **Orange** circle (top apex) = predicate/relationship

Connected by faint edges. The colours match the fact card colour coding in the playground.

At favicon size (16px), simplify to three dots + two edges (drop the base edge).
