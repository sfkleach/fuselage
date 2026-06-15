#!/usr/bin/env python3
"""
Decision Records Management Script
----------------------------------

This script helps manage decision records in a structured format. It provides
functionality to initialize a decisions directory with a template and to add
new decision records incrementally numbered.

**Purpose:**

- **Initialize Decision Records Directory:** Sets up a `docs/decisions`
  directory containing a `decision-template.md` file that serves as a template
  for all decision records.
- **Add Decision Records:** Adds new decision records with a unique serial
  number at the start of the folder and file names.

**Numbering Scheme:**

- Decision records are stored in subfolders within `docs/decisions`.
- Each subfolder and its markdown file are prefixed with an incrementing serial
  number starting from `0000`, e.g., `0000-decision-topic`.
- The script scans existing folders to determine the next available number,
  ensuring no overlaps.

**Usage:**

- Initialization
  - Command: `python3 scripts/decisions.py --init`
  - Creates the docs/decisions directory and adds the decision-template.md file.

- Adding a Decision
  - Command: `python3 scripts/decisions.py --add "Your Decision Topic"`
  - Generates a new subfolder and markdown file with the appropriate numbering, 
    replacing placeholders in the template with actual values.

**Customization**

After a decision folder is generated, the template file can be freely customized.
You can add or modify content as needed for your specific decision record.
Each decision record has its own subfolder, allowing you to store accompanying
information, resources, or files related to that decision.

**The TEMPLATE constant is a seed, not a synced copy**

The TEMPLATE string in this script and the `decision-template.md` that `--init`
writes into a project are independent by design. TEMPLATE is only the *seed*:
`--init` stamps it out once, and from then on each project is expected to edit
its own `decision-template.md` to suit its needs. The two are therefore *meant*
to drift apart. Do not add a parity/drift check between them — that would fight
the intended per-project customization. Keep TEMPLATE itself a sensible,
self-consistent starting point; do not try to keep it equal to any project's
evolved copy.


The template supports placeholders that are automatically replaced when adding 
a new decision:

- {{Decision Title}}: Replaced with the decision topic provided.
- {{YYYY-MM-DD}}: Replaced with the current date.

If further placeholder processing is required, you can achieve this by updating 
the PLACEHOLDER_METHODS mapping and adding corresponding methods to the 
ProcessTemplate class.

For example, to add a {{Decision Number}} placeholder, you would:

- Add '{{Decision Number}}': 'decision_number' to PLACEHOLDER_METHODS.
- Add a decision_number method to ProcessTemplate to handle the substitution.
"""

import argparse
from pathlib import Path
import re
from datetime import datetime

# Top-level string variable TEMPLATE
TEMPLATE = """# {{RecordID}} - {{Decision Title}}, {{YYYY-MM-DD}}

<!--
This template covers two kinds of decision. Pick one and delete the other's
guidance before writing.

  Fork — a decision *between* alternatives. A genuine branch point with two or
         more live, comparable options on the table. Use the "Options" and
         "Pros and Cons" sections to compare them.

  Leap — a *commitment* to a design in the absence of real alternatives. A
         coherent approach presented itself and you ran with it; the
         alternatives were unknown, not yet invented, or never live candidates.
         A Leap is NOT a degenerate Fork — do not manufacture straw-man options
         to fill the comparison sections. Omit them (CLAUDE.md permits this) and
         document the constraints that forced the design and, above all, what
         you knowingly gave up by committing. That accepted cost is the
         load-bearing part of the record.

The pros/cons format is a *prompt* — "were there real alternatives?" — not a
*form* to be filled. If asking surfaces a genuine fork, write a Fork. If it
doesn't, write a Leap and say so plainly.

Mark post-hoc records (written after the fact) as such in the Issue section.
-->

## Issue

Describe the problem and the decision to be made. State whether this is a
**Fork** or a **Leap**, and whether the record is post-hoc.

## Factors

List the factors and constraints that bear on the decision.

## Decision

State the outcome and the reasoning behind it.

<!-- For a LEAP, make the trade-off explicit here or in Consequences: what does
     this design buy, and what does committing to it give up? -->

## Consequences

The impact of the decision — including the costs knowingly accepted.

<!-- ─────────────────────────────────────────────────────────────────────
     FORK ONLY — delete this whole block for a Leap.
     ───────────────────────────────────────────────────────────────────── -->

## Options and Outcome

List the alternatives considered and which was chosen.

## Pros and Cons of Options

### Option 1
- Pros
- Cons
- Interesting

### Option 2
Etc ...

<!-- ─────────────────────────── end Fork-only block ─────────────────────── -->

## Additional Notes
"""

# Top-level mapping of placeholders to method names (as strings)
PLACEHOLDER_METHODS = {
    '{{Decision Title}}': 'decision_title',
    '{{YYYY-MM-DD}}': 'yyyy_mm_dd',
    '{{RecordID}}': 'record_id',
}

class ProcessTemplate:
    """Class to process template placeholders."""

    def __init__(self, args, record_id):
        """Initialize with argparse args."""
        self._args = args
        self._record_id = record_id

    def decision_title(self, match):
        """Replace {{Decision Title}} with the actual decision title."""
        return self._args.add

    def yyyy_mm_dd(self, match):
        """Replace {{YYYY-MM-DD}} with the current date."""
        return datetime.now().strftime('%Y-%m-%d')
    
    def record_id(self, match):
        """Replace {{RecordID}} with the record ID."""
        return f"{self._record_id:04}"

class DecisionRecords:
    def initialize(self):
        """Initialize the decisions directory and template."""
        # Create the folder 'docs/decisions'
        decisions_dir = Path('docs/decisions')
        decisions_dir.mkdir(parents=True, exist_ok=True)
        # Create the template file 'decision-template.md'
        template_path = decisions_dir / 'decision-template.md'
        template_path.write_text(TEMPLATE)
        print(f"Initialization complete: Template file created at '{template_path}'")

    def add_decision(self, topic, args):
        """Add a new decision record."""
        # Sanitize the topic string
        safe_topic = self._sanitize_topic(topic)

        # Ensure the decisions directory exists
        decisions_dir = Path('docs/decisions')
        if not decisions_dir.is_dir():
            print(f"The directory '{decisions_dir}' does not exist. Please run '--init' first.")
            return

        # Get the next available number
        next_number = self._get_next_number(decisions_dir)

        # Format the number with leading zeros (e.g., '0000')
        number_str = f"{next_number:04}"

        # Create the folder name and file name with the number prefixed
        folder_name = f"{number_str}-{safe_topic}"
        decision_folder = decisions_dir / folder_name
        decision_folder.mkdir(parents=True, exist_ok=True)

        decision_file = decision_folder / f"{number_str}-{safe_topic}.md"

        # Path to the template file
        template_path = decisions_dir / 'decision-template.md'

        # Check if the template file exists
        if not template_path.is_file():
            print(f"Template file not found at '{template_path}'. Please run '--init' first.")
            return

        # Read the template content
        template_content = template_path.read_text()

        # Process the template to replace placeholders
        template_processor = ProcessTemplate(args, next_number)
        template_filled = self._process_template(template_content, template_processor)

        # Write the filled template to the new decision file
        decision_file.write_text(template_filled)

        print(f"Decision '{topic}' added at '{decision_file}'")

    def _get_next_number(self, decisions_dir):
        """Private method to find the next available decision number."""
        existing_numbers = []

        # Scan existing subfolders to find the highest number
        for item in decisions_dir.iterdir():
            if item.is_dir():
                # Match folders that start with a number followed by a hyphen
                match = re.match(r'^(\d+)-.+$', item.name)
                if match:
                    number = int(match.group(1))
                    existing_numbers.append(number)

        if existing_numbers:
            next_number = max(existing_numbers) + 1
        else:
            next_number = 0  # Start numbering from 0

        return next_number

    def _sanitize_topic(self, topic):
        """Private method to sanitize the topic string for use in file names."""
        # Remove any characters that are not word characters, spaces, or hyphens
        safe_topic = re.sub(r'[^\w\s-]', '', topic).strip().lower()
        # Replace one or more whitespace characters with a single hyphen
        safe_topic = re.sub(r'\s+', '-', safe_topic)
        return safe_topic

    def _process_template(self, template_content, processor):
        """Private method to process the template content and replace placeholders."""
        # Regex pattern to match placeholders
        pattern = re.compile(r'{{[^{}]+}}')

        # Function to replace each placeholder
        def replace_placeholder(match):
            placeholder = match.group(0)
            method_name = PLACEHOLDER_METHODS.get(placeholder)
            if method_name:
                # Use getattr to get the method from the processor instance
                method = getattr(processor, method_name, None)
                if method:
                    return method(match)
                else:
                    print(f"Warning: Method '{method_name}' not found in ProcessTemplate.")
                    return placeholder
            else:
                # Return the original placeholder if no method is found
                return placeholder

        # Perform the substitution
        return pattern.sub(replace_placeholder, template_content)

def main():
    parser = argparse.ArgumentParser(description="Manage decision records")
    parser.add_argument('--init', action='store_true', help="Initialize the decision records")
    parser.add_argument('--add', type=str, help="Add a new decision with the given TOPIC", metavar='TOPIC')

    args = parser.parse_args()

    decision_records = DecisionRecords()

    if args.init:
        decision_records.initialize()

    if args.add:
        decision_records.add_decision(args.add, args)

if __name__ == '__main__':
    main()