"""Keep central Runtime CI staged until isolated execution is admitted."""

from pathlib import Path
import re
import unittest


WORKFLOW = Path('.github/workflows/central-runtime-ci.yml')
PIN = '29c14c67fd2a49e3719bfc0d3c1b9cad9531e33f'


class CentralRuntimeCallerContract(unittest.TestCase):
    """Preserve fixed central source and no-secret operator activation."""

    def test_caller_is_immutable_and_has_no_hosted_job_or_secret(self):
        """The consumer only calls the reviewed central callee."""
        source = WORKFLOW.read_text(encoding='utf-8')
        self.assertIn(
            f'uses: ContextualWisdomLab/.github/.github/workflows/psychometrics-commons-ci.yml@{PIN}',
            source,
        )
        self.assertEqual(len(re.findall(r'^    uses:', source, re.MULTILINE)), 1)
        self.assertNotIn('runs-on:', source)
        self.assertNotIn('secrets:', source)
        self.assertNotIn('pull_request_target:', source)
        self.assertNotIn('workflow_dispatch:', source)
        self.assertNotIn('push:', source)
        self.assertIn('contents: read', source)

    def test_same_repo_ready_pr_and_operator_admission_are_mandatory(self):
        """Absent admission cannot run product code on a shared worker."""
        source = WORKFLOW.read_text(encoding='utf-8')
        for guard in (
            "vars.PSYCHOMETRICS_ISOLATED_CI_ADMITTED == 'true'",
            'github.event.pull_request.draft == false',
            'github.event.pull_request.head.repo.full_name == github.repository',
            'github.event.pull_request.base.repo.full_name == github.repository',
            "github.event.pull_request.base.ref == 'main'",
        ):
            self.assertIn(guard, source)
        self.assertIn('cancel-in-progress: true', source)
        self.assertIn('github.event.pull_request.number', source)
        self.assertNotIn('continue-on-error:', source)


if __name__ == '__main__':
    unittest.main()
