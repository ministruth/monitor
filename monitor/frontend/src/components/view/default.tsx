import GeoIP from '@/common/components/geoip';
import Table from '@/common_components/layout/table';
import {
  IDColumn,
  SearchColumn,
  StatusColumn,
} from '@/common_components/layout/table/column';
import styles from '@/common_components/layout/table/style.less';
import TableBtn from '@/common_components/layout/table/tableBtn';
import { API_PREFIX, PLUGIN_ID } from '@/config';
import { UserPerm, getAPI, getIntl } from '@/utils';
import { CheckOutlined, CloseOutlined, CodeOutlined } from '@ant-design/icons';
import { ParamsType, ProDescriptions } from '@ant-design/pro-components';
import { ProColumns } from '@ant-design/pro-table';
import { SortOrder } from 'antd/es/table/interface';
import bytes from 'bytes';

const request = async (params?: ParamsType, _?: Record<string, SortOrder>) => {
  const msg = await getAPI(`${API_PREFIX}/agents`, {
    text: params?.text,
    status: params?.status,
    page: params?.current,
    size: params?.pageSize,
  });
  return {
    data: msg.data.data,
    success: true,
    total: msg.data.total,
  };
};

export interface TabItemProps {
  addTabCallback?: (row: any) => void;
}

const DefaultTab: React.FC<TabItemProps> = (props) => {
  const intl = getIntl();
  const statusEnum: { [Key: number]: { label: string; color: string } } = {
    0: {
      label: intl.get('tables.status.offline'),
      color: 'default',
    },
    1: {
      label: intl.get('tables.status.online'),
      color: 'success',
    },
    2: {
      label: intl.get('tables.status.updating'),
      color: 'warning',
    },
  };
  const columns: ProColumns[] = [
    SearchColumn(intl),
    IDColumn(intl),
    {
      title: intl.get('tables.name'),
      dataIndex: 'name',
      align: 'center',
      hideInSearch: true,
      ellipsis: true,
      onCell: () => {
        return {
          style: {
            maxWidth: 200,
          },
        };
      },
    },
    {
      title: intl.get('tables.ip'),
      dataIndex: 'ip',
      align: 'center',
      hideInSearch: true,
      render: (_, row) => <GeoIP value={row.ip} />,
    },
    {
      title: intl.get('tables.os'),
      dataIndex: 'os',
      align: 'center',
      hideInSearch: true,
    },
    {
      title: intl.get('tables.arch'),
      dataIndex: 'arch',
      align: 'center',
      hideInSearch: true,
    },
    StatusColumn(intl.get('tables.status'), 'status', statusEnum),
    {
      title: intl.get('tables.cpu'),
      dataIndex: 'cpu',
      valueType: 'percent',
      align: 'center',
      hideInSearch: true,
    },
    {
      title: intl.get('tables.memory'),
      valueType: 'percent',
      align: 'center',
      renderText: (_, row) => `${(row.memory * 100) / row.total_memory}`,
      hideInSearch: true,
    },
    {
      title: intl.get('tables.latency'),
      dataIndex: 'latency',
      align: 'center',
      hideInSearch: true,
      renderText: (text) => (text > 9999 ? '9999 ms' : (text ?? '-') + ' ms'),
    },
    {
      title: intl.get('app.op'),
      valueType: 'option',
      align: 'center',
      className: styles.operation,
      width: 100,
      render: (_, row) => {
        return [
          <TableBtn
            key="shell"
            icon={CodeOutlined}
            tip={intl.get('pages.view.card.shell.tip')}
            onClick={(_) => props.addTabCallback?.(row)}
            permName={`view.${PLUGIN_ID}`}
            perm={UserPerm.PermAll}
            disabled={row.status != 1 || row.disable_shell}
          />,
        ];
      },
    },
  ];

  return (
    <Table
      rowKey="id"
      request={request}
      columns={columns}
      poll={true}
      expandable={{
        expandRowByClick: true,
        expandedRowRender: (record: any) => {
          return (
            <ProDescriptions
              column={3}
              dataSource={record}
              contentStyle={{ alignItems: 'center' }}
              columns={[
                {
                  title: intl.get('tables.uid'),
                  dataIndex: 'uid',
                  style: { paddingBottom: 0 },
                  copyable: true,
                },
                {
                  title: intl.get('tables.hostname'),
                  dataIndex: 'hostname',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.system'),
                  dataIndex: 'system',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.lastlogin'),
                  dataIndex: 'last_login',
                  valueType: 'dateTime',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.lastrsp'),
                  dataIndex: 'last_rsp',
                  valueType: 'dateTime',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.memory'),
                  renderText: (_, row) =>
                    `${bytes.format(row.memory, { unitSeparator: ' ' }) ?? '-'} / ${bytes.format(row.total_memory, { unitSeparator: ' ' }) ?? '-'}`,
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.disk'),
                  renderText: (_, row) =>
                    `${bytes.format(row.disk, { unitSeparator: ' ' }) ?? '-'} / ${bytes.format(row.total_disk, { unitSeparator: ' ' }) ?? '-'}`,
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.network'),
                  renderText: (_, row) =>
                    `${bytes.format(row.net_up, { unitSeparator: ' ' }) ?? '-'}/s ↑ | ${bytes.format(row.net_down, { unitSeparator: ' ' }) ?? '-'}/s ↓`,
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.bandwidth'),
                  renderText: (_, row) =>
                    `${bytes.format(row.band_up, { unitSeparator: ' ' }) ?? '-'} ↑ | ${bytes.format(row.band_down, { unitSeparator: ' ' }) ?? '-'} ↓`,
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.address'),
                  dataIndex: 'address',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.endpoint'),
                  dataIndex: 'endpoint',
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.report_rate'),
                  dataIndex: 'report_rate',
                  renderText: (_, row) =>
                    `${(row.report_rate ?? 0) == 0 ? '-' : row.report_rate + ' s'}`,
                  style: { paddingBottom: 0 },
                },
                {
                  title: intl.get('tables.disable_shell'),
                  dataIndex: 'disable_shell',
                  render: (_, row) => {
                    if (row.status !== 1) return '-';
                    else if (row.disable_shell)
                      return <CloseOutlined style={{ color: '#f5222d' }} />;
                    else return <CheckOutlined style={{ color: '#52c41a' }} />;
                  },
                  style: { paddingBottom: 0 },
                },
              ]}
            />
          );
        },
      }}
    />
  );
};

export default DefaultTab;
